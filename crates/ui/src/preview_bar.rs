//! The one toolbar every preview fills.
//!
//! A preview never builds a row: it lists its tools, each in a slot, and this
//! module lays them out, so the same action is in the same place whatever is
//! shown. Slots, left to right: contents (its drawer opens on that side),
//! navigate (previous and next, page), view (how the content is shown:
//! rendered or source, zoom or text size, fit), a spacer, find, More, then
//! the × that closes a member of a container (`close_tool`). A member shown
//! under its container's view starts with its name (`Slot::Title`). Copy has
//! no button: it is Ctrl+C and the content's own right-click menu, where
//! people look for it.
//!
//! The menu rule is every responsive toolbar's: More holds what has no button
//! in the variant the row chose this frame (`PopoverAnchor::popup_for`), plus
//! what never has one (open externally, copy the path, a kind's own settings).
//! Nothing is both a button and a menu row. More is there whenever it holds
//! something, last but for the ×, which stays at the end. Open externally and
//! copy the path are the entry's: under a container's view they are in its
//! More, not the member's (`Bar::always`).
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
    pub const FIND: &str = "Find";
    pub const SOURCE: &str = "Source";
    pub const HEX: &str = "View as hex";
    pub const ZOOM: &str = "Zoom and fit";
    pub const FULLSCREEN: &str = "Fullscreen";
    pub const MORE: &str = "More";
}

/// How tall the bar is.
pub const HEIGHT: f32 = 42.;

/// `keep` of an item no variant shows.
const NEVER: usize = usize::MAX;

/// The popup More opens; a tool names another with [`Item::opens`].
pub const MORE: &str = "preview";

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Slot {
    /// The name of a member shown under its container's view, before
    /// everything: the card names the container.
    Title,
    /// Contents, first of the tools: the drawer it opens is on the left too.
    Lead,
    Navigate,
    View,
    /// After the spacer, in the order given.
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

    /// An action too rare for a button in any variant: always a row of More.
    pub fn menu_only(button: Button) -> Self {
        Item {
            slot: Slot::Tools,
            keep: 0,
            tool: Tool::Button(button),
            opens: None,
            fills: false,
        }
        .never()
    }

    fn never(mut self) -> Self {
        self.keep = NEVER;
        self
    }

    fn shown(&self, mode: usize) -> bool {
        self.keep != NEVER && self.keep >= mode
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
    /// Whether More holds what is about the entry (open externally, copy
    /// the path): every bar's but a member's under its container's view,
    /// whose bar has them.
    pub always: bool,
    /// The × at the end that leaves a member (`close_tool`).
    pub close: Option<Button>,
}

impl Bar {
    pub fn new(id: &'static str) -> Self {
        Bar {
            id,
            items: Vec::new(),
            settings: None,
            popups: Vec::new(),
            always: true,
            close: None,
        }
    }

    pub fn push(&mut self, item: Item) {
        self.items.push(item);
    }
}

/// Where a document or a book is, between its previous and next buttons:
/// its page or chapter as a button that becomes a field when clicked
/// (`edit`), then how many there are. `editor` finds the field while it is
/// open; the reader's page and a book's chapter are the same tool. `unit`,
/// when a document can be counted two ways (chapters or printed pages),
/// makes the count the picker of that unit, opening the popup it names.
pub fn number_tool(
    bar: &mut Bar,
    (id, title): (&'static str, &'static str),
    edit: (&'static str, serde_json::Value),
    (value, total): (String, String),
    editor: fn(&Shell) -> Option<&Field>,
    unit: Option<(Button, &'static str)>,
    theme: &Theme,
) {
    let (accent, border, paper, muted) = (theme.accent, theme.border, theme.bg, theme.text_muted);
    bar.push(Item::widget(
        Slot::Navigate,
        2,
        move |shell, cx| match editor(shell) {
            Some(field) => div()
                .w(px(44.))
                .flex_none()
                .h(px(28.))
                .px_1()
                .border_1()
                .border_color(accent)
                .rounded_md()
                .bg(paper)
                .child(field.input.clone())
                .into_any_element(),
            None => Button::new(id, "", title, edit.0)
                .args(edit.1.clone())
                .label(value.clone())
                .render(shell, cx)
                .bg(paper)
                .border_color(border)
                .into_any_element(),
        },
    ));
    if let Some((button, popup)) = unit {
        bar.push(Item::button(Slot::Navigate, 1, button.label(format!("/ {total}"))).opens(popup));
        return;
    }
    bar.push(Item::widget(Slot::Navigate, 1, move |_, _| {
        div()
            .flex_none()
            .text_xs()
            .text_color(muted)
            .whitespace_nowrap()
            .child(format!("/ {total}"))
            .into_any_element()
    }));
}

/// The count's own picker (`number_tool`): count by the document's own
/// unit (`plain`: chapters of a book, pages of a document) or by the pages
/// of its printed edition (`ui.printed_pages`).
pub fn unit_popup(plain: &'static str) -> Popup {
    Rc::new(move |shell, theme, cx| {
        let popup = shell.preview.menu.as_ref()?;
        let mut choices = div().flex().flex_col().gap_1();
        for (id, label, on) in [
            ("unit-plain", plain, false),
            ("unit-printed", "Printed pages", true),
        ] {
            choices = choices.child(
                Button::new(id, "", label, "ui.printed_pages")
                    .label(label)
                    .args(serde_json::json!({"on":on}))
                    .active(shell.prefs.printed_pages == on)
                    .menu()
                    .render(shell, cx),
            );
        }
        Some(
            toolbar::popup_panel(popup, "ui.preview_menu", theme, cx)
                .child(choices)
                .into_any_element(),
        )
    })
}

/// The way out of a member, the same whatever the member turned out to be:
/// an × at the end of its bar. Under its container's view (`members.rs`)
/// the bar also starts with the member's name, which the card no longer
/// says, and leaves what is about the entry to the container's More.
pub fn close_tool(bar: &mut Bar, shell: &Shell, theme: &Theme) {
    if !shell.preview.inside() {
        return;
    }
    // Its tooltip names Escape where Escape does the same (`keymap::MEMBER`).
    bar.close = Some(
        Button::new("preview-close", "close", "Close", "ui.inspect")
            .args(serde_json::json!({"action":"back"})),
    );
    let Some((name, size)) = crate::members::title(shell) else {
        return;
    };
    bar.always = false;
    let (text, muted) = (theme.text, theme.text_muted);
    let font = shell.font.clone();
    bar.push(
        Item::widget(Slot::Title, 0, move |_, _| {
            div()
                .flex_1()
                .min_w(px(64.))
                .pl_1()
                .flex()
                .items_baseline()
                .gap_1()
                .overflow_hidden()
                .whitespace_nowrap()
                .child(crate::elements::clip::clipped(
                    div()
                        .id("preview-member-name")
                        .min_w_0()
                        .overflow_hidden()
                        .text_ellipsis_middle()
                        .text_sm()
                        .text_color(text),
                    name.clone(),
                    font.clone(),
                ))
                .children(
                    size.clone()
                        .map(|size| div().flex_none().text_xs().text_color(muted).child(size)),
                )
                .into_any_element()
        })
        .fills(),
    );
}

/// The whole screen, the same tool at the end of every kind's bar, lit
/// while on (`ui.preview_fullscreen`), there while the next file loads.
/// There the bar starts with the name of what is shown, which the card no
/// longer says, unless a member's name is already there (`close_tool`).
pub fn fullscreen_tool(bar: &mut Bar, shell: &Shell, theme: &Theme) {
    let on = shell.preview.fullscreen.is_some();
    if !on && shell.fullscreen_shows().is_none() {
        return;
    }
    bar.push(Item::button(
        Slot::Tools,
        1,
        Button::new(
            "preview-fullscreen",
            "fullscreen",
            label::FULLSCREEN,
            "ui.preview_fullscreen",
        )
        .active(on),
    ));
    if !on || bar.items.iter().any(|item| item.slot == Slot::Title) {
        return;
    }
    let (name, text) = (shell.preview.info.name.clone(), theme.text);
    let font = shell.font.clone();
    bar.push(
        Item::widget(Slot::Title, 0, move |_, _| {
            div()
                .flex_1()
                .min_w(px(64.))
                .pl_1()
                .flex()
                .items_center()
                .overflow_hidden()
                .whitespace_nowrap()
                .child(crate::elements::clip::clipped(
                    div()
                        .id("preview-fullscreen-name")
                        .min_w_0()
                        .overflow_hidden()
                        .text_ellipsis_middle()
                        .text_sm()
                        .text_color(text),
                    name.clone(),
                    font.clone(),
                ))
                .into_any_element()
        })
        .fills(),
    );
}

/// Whether More has anything to hold in variant `mode`: what the entry is
/// about, a kind's settings, or a button this variant drops.
fn has_more(bar: &Bar, mode: usize) -> bool {
    bar.always
        || bar.settings.is_some()
        || bar
            .items
            .iter()
            .any(|item| !item.shown(mode) && matches!(item.tool, Tool::Button(_)))
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

pub(crate) fn separator(theme: &Theme) -> Div {
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
    let filled = bar.items.iter().any(|item| item.fills && item.shown(mode));
    let mut items: Vec<&Item> = bar.items.iter().filter(|item| item.shown(mode)).collect();
    items.sort_by_key(|item| item.slot);
    for item in items {
        if last.is_some_and(|slot| slot != item.slot) {
            row = if item.slot == Slot::Tools && !filled {
                row.child(div().flex_1().min_w_0())
            } else if item.slot != Slot::Tools {
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
    row.when(has_more(bar, mode), |row| {
        row.child(
            Button::new("preview-more", "more", label::MORE, "ui.preview_menu")
                .active(active == Some(MORE))
                // A popup whose own button this variant dropped opens here.
                .anchor_if(anchor, active.is_some() && !anchored)
                .render(shell, cx),
        )
    })
    .children(bar.close.clone().map(|close| close.render(shell, cx)))
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
        height: HEIGHT,
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
                for item in bar.items.iter().filter(|item| !item.shown(mode)) {
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
                if dropped && bar.always {
                    panel = panel.child(separator(theme));
                }
                panel
                    .when(bar.always, |panel| panel.children(always(shell, cx)))
                    .into_any_element()
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

/// A text field of a preview's bar (find, a page number): Enter and Escape
/// run commands of the shell, so a field does what its buttons do.
pub struct Field {
    pub input: gpui::Entity<crate::input::TextInput>,
    _subscription: gpui::Subscription,
}

impl Field {
    pub fn new(
        placeholder: &'static str,
        submit: &'static str,
        cancel: &'static str,
        window: &mut gpui::Window,
        cx: &mut Context<Shell>,
    ) -> Self {
        use crate::input::{InputEvent, TextInput};
        let input = cx.new(|cx| TextInput::new(placeholder, cx));
        let subscription = cx.subscribe_in(
            &input,
            window,
            move |this, _, event: &InputEvent, window, cx| {
                let (command, args) = match event {
                    InputEvent::Submit => split(submit),
                    InputEvent::Cancel => split(cancel),
                    InputEvent::Changed => return cx.notify(),
                };
                this.run(command, args, window, cx);
            },
        );
        Field {
            input,
            _subscription: subscription,
        }
    }

    pub fn text(&self, cx: &gpui::App) -> String {
        self.input.read(cx).text().to_owned()
    }

    pub fn is_focused(&self, window: &gpui::Window, cx: &gpui::App) -> bool {
        self.input.read(cx).is_focused(window)
    }
}

/// `"ui.inspect find_next"`: a command and the `action` it is given, if any.
fn split(command: &'static str) -> (&'static str, serde_json::Value) {
    match command.split_once(' ') {
        Some((command, action)) => (command, serde_json::json!({ "action": action })),
        None => (command, serde_json::json!({})),
    }
}

/// The find field under its button, the same in the reader and in a
/// structured preview: the field, a line saying where the search stands,
/// and the buttons that step through the matches and close it. `context`
/// names its keys (`keymap::PREVIEW_FIND`).
pub fn find_popup(
    field: &Field,
    context: &'static str,
    status: String,
    buttons: Vec<Button>,
    shell: &Shell,
    theme: &Theme,
    cx: &mut Context<Shell>,
) -> AnyElement {
    div()
        .tab_group()
        .key_context(context)
        .w(px(280.))
        .p_2()
        .flex()
        .flex_col()
        .gap_2()
        .rounded_lg()
        .border_1()
        .border_color(theme.border)
        .bg(theme.bg)
        .shadow_lg()
        .occlude()
        .child(
            div()
                .px_2()
                .py_1()
                .border_1()
                .border_color(theme.accent)
                .rounded_md()
                .child(field.input.clone()),
        )
        .child(
            div()
                .flex()
                .items_center()
                .gap_1()
                .text_xs()
                .child(div().flex_1().text_color(theme.text_muted).child(status))
                .children(buttons.into_iter().map(|button| button.render(shell, cx))),
        )
        .into_any_element()
}
