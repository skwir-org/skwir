//! The one toolbar every preview fills.
//!
//! A preview never builds a row: it lists its tools, each in a slot, and this
//! module lays them out, so the same action is in the same place whatever is
//! shown. Slots, left to right: contents (its drawer opens on that side),
//! navigate (previous and next, page), view (zoom or text size, fit), a
//! spacer, find and the whole screen, how the file is shown (`look_tools`:
//! Source, 3D, Full preview, View as hex), More, then the × that closes a
//! member of a container (`close_tool`). How the file is shown is held to
//! the bar's end, beside More, which every bar has: what stands after it
//! is the same in every way of looking, so a click never moves it. A member shown
//! under its container's view starts with its name (`Slot::Title`). Copy has
//! no button: it is Ctrl+C and the content's own right-click menu, where
//! people look for it.
//!
//! The menu rule is every responsive toolbar's: More holds what has no button
//! in the variant the row chose this frame (`PopoverAnchor::popup_for`), plus
//! what never has one (open externally, copy the path, a kind's own settings).
//! Nothing is both a button and a menu row. More is in every bar, last but
//! for the ×, which stays at the end, and disabled where it holds nothing
//! (a member's, whose entry's rows are its container's). Open externally and
//! copy the path are the entry's: under a container's view they are in its
//! More, not the member's (`Bar::always`). Another way of looking at the
//! file (View as hex) is last, after them (`Bar::otherwise`).
use crate::sizes::TextSizes;
use crate::sizes::sized;
use std::rc::Rc;

use gpui::{AnyElement, Context, Div, div, prelude::*, px};

use crate::elements::{
    adaptive::{AdaptiveRow, Chosen},
    popover::PopoverAnchor,
};
use crate::shell::Shell;
use crate::theme::Theme;
use crate::toolbar::{self, Button};
use skwir_core::registry::MenuItem;

/// The labels every preview uses for the actions they share.
pub mod label {
    pub const FIT: &str = "Fit";
    pub const FIT_WIDTH: &str = "Fit width";
    pub const CONTENTS: &str = "Contents";
    pub const FIND: &str = "Find";
    pub const SOURCE: &str = "Source";
    /// A model turned in 3D (ADR-067).
    pub const TURN: &str = "3D";
    pub const HEX: &str = "View as hex";
    /// All of a file a preview cut short or refused to stay quick.
    pub const WHOLE: &str = "Full preview";
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
    /// Contents, first of the tools: the drawer it opens is on the left
    /// too.
    Lead,
    Navigate,
    View,
    /// After the spacer, in the order given: find, the whole screen.
    Tools,
    /// How the file is shown (`look_tools`: Source, 3D, Full preview, View
    /// as hex), last before More: what stands after it (More, the ×) is
    /// there in every way of looking, so nothing moves it when it is
    /// clicked, while the tools before it change with how the file is
    /// shown.
    Look,
}

/// A piece of a row that is not a command button: a page field, a time, a
/// seek bar. It is built once per variant and never appears in the menu.
pub type Widget = Rc<dyn Fn(&Shell, &mut Context<Shell>) -> AnyElement>;

// A bar's few tools, made for one frame and drawn at once: boxing each
// button would allocate more than the widgets' room costs.
#[allow(clippy::large_enum_variant)]
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
    /// Another way of looking at the file, which leaves this one (View as
    /// hex): the end of More, after what is about the entry, as a view a
    /// menu opens comes after copying and making (ADR-099).
    otherwise: Vec<Button>,
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
            otherwise: Vec::new(),
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
                .w(sized(44.))
                .flex_none()
                .h(sized(28.))
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
            .text_body()
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
            toolbar::menu_panel(popup, "ui.preview_menu", theme, cx)
                .child(choices)
                .into_any_element(),
        )
    })
}

/// What takes the room the tools leave, and only that (a member's or a
/// file's name, a section's caption): its natural width is its least,
/// 64 px at 100 % (`sized`), so it never decides which tools fit (`AdaptiveRow`
/// measures each variant at its natural width), and it is cut where it
/// stands from its first frame, whatever comes beside it later. Its basis
/// is a length: a percentage of no width (`flex_1`'s 0 %), as a bar is
/// measured, counts as the whole text, which a long name made too wide
/// for any variant that held it.
/// A name's shares of the bar's spare room, to a caption's one: what a
/// name says is cut first where it is long.
const NAMED: f32 = 3.;

pub fn spare() -> Div {
    div()
        .flex_grow(1.)
        .flex_shrink(1.)
        .flex_basis(px(0.))
        .min_w(sized(64.))
}

/// The way out of a member, the same whatever the member turned out to be:
/// an × at the end of its bar. Under its container's view (`members.rs`)
/// the bar also starts with the member's name, which the card no longer
/// says, once something of the member is shown, and leaves what is about
/// the entry to the container's More. The file shown for its folder (its
/// README) has in its More where the names it was found by are chosen.
pub fn close_tool(bar: &mut Bar, shell: &Shell, theme: &Theme) {
    if !shell.preview.inside() {
        return;
    }
    // Its tooltip names Escape where Escape does the same (`keymap::MEMBER`).
    bar.close = Some(
        Button::new("preview-close", "close", "Close", "ui.inspect")
            .args(serde_json::json!({"action":"back"})),
    );
    // The file shown for its folder, whatever it turns out to be, says
    // where the names it was found by are chosen.
    if shell.preview.stands_for().is_some() {
        bar.push(Item::menu_only(
            Button::new(
                "preview-stand-ins",
                "settings",
                "Files shown for folders…",
                "ui.settings",
            )
            .args(serde_json::json!({ "key": crate::settings::FOLDER_PREVIEW_NAMES_KEY })),
        ));
    }
    // Named once something of it is shown: a bar of nothing yet has the
    // room its tools will take, and a name drawn there long, then cut as
    // they came, flickered.
    if shell.preview.content.is_none() {
        return;
    }
    let Some((name, size)) = crate::members::title(shell) else {
        return;
    };
    bar.always = false;
    let (text, muted) = (theme.text, theme.text_muted);
    bar.push(
        Item::widget(Slot::Title, 0, move |_, _| {
            // A name never decides which tools fit, nor gives way as a
            // caption comes (lines counted): cut at once, at the room it
            // keeps, three shares of the spare room to a caption's one.
            spare()
                .flex_grow(NAMED)
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
                        .text_body()
                        .text_color(text),
                    name.clone(),
                ))
                .children(
                    size.clone()
                        .map(|size| div().flex_none().text_small().text_color(muted).child(size)),
                )
                .into_any_element()
        })
        .fills(),
    );
}

/// How a file is shown, which the tools after it belong to: each a tool
/// lit while it is on, where a click shows the file as it was shown before.
/// Each is `None` where the bar has no such tool.
#[derive(Default)]
pub struct Looks {
    /// Source: the file's own text, lit while shown (`InspectRequest::source`).
    pub source: Option<bool>,
    /// 3D: the model turned, lit while it turns.
    pub turn: Option<bool>,
    /// Full preview: all of a file a preview cut short or refused to stay
    /// quick, read again past those bounds (`InspectRequest::whole`), for
    /// this file only: lit while on, and disabled where it would show no
    /// more rather than gone (`Whole::enabled`).
    pub whole: Option<Whole>,
    /// View as hex: lit while the bytes are shown by choice, where a click
    /// shows the file as it would be shown again (`ui.inspect`'s `auto`).
    pub hex: Option<Hex>,
    /// The other ways of looking at the entry its providers offer (its
    /// history, `PreviewProvider::way`), first of the group, each lit
    /// while shown (`ways`).
    pub ways: Vec<WayLook>,
}

/// A way of looking a provider offers, as its button shows it.
pub struct WayLook {
    pub id: &'static str,
    pub way: skwir_core::preview::Way,
    pub on: bool,
}

/// The ways of looking at the entry, for the bar being built: the
/// entry's own (the panel's while it shows the entry or the file standing
/// for a folder; a container's kept above a member when it is the entry),
/// never a member's. Lit where `shown` (the provider what is shown was
/// asked of) is theirs.
pub fn ways(shell: &Shell, parked: bool, shown: Option<&str>) -> Vec<WayLook> {
    let preview = &shell.preview;
    let entry = if parked {
        preview.parked().is_some_and(|parked| parked.depth == 0)
    } else {
        preview.parked().is_none() && (!preview.inside() || preview.stands_for().is_some())
    };
    if !entry {
        return Vec::new();
    }
    preview
        .ways
        .iter()
        .map(|(id, way)| WayLook {
            id,
            way: *way,
            on: shown == Some(*id),
        })
        .collect()
}

#[derive(Clone, Copy)]
pub struct Whole {
    pub on: bool,
    pub enabled: bool,
}

#[derive(Clone, Copy)]
pub enum Hex {
    /// Lit, the way back from the bytes.
    Shown,
    /// A button: nothing else of the file is shown, nor can be.
    Button,
    /// A row of More, as an action too rare for a button: the last
    /// (`Bar::otherwise`).
    Row,
}

/// How the file is shown, at the end of every bar that has any of it, just
/// before More (`Slot::Look`), in this order: Source, 3D, Full preview,
/// View as hex. The tools before it belong to how the file is shown (a
/// picture's zoom, a text's size, a model's look, find) and change with
/// it; what stands after it (More, the ×) does not, so a tool clicked is
/// where it was once it is lit, the way back where the way in was, and
/// the bytes' lit button beside the More they were asked from. One
/// function pushes them all, so the order is the same in every kind's
/// bar.
pub fn look_tools(bar: &mut Bar, looks: Looks) {
    let tool = |id, icon, title, args: serde_json::Value, label: &'static str, on: bool| {
        Button::new(id, icon, title, "ui.inspect")
            .args(args)
            .label(label)
            .active(on)
    };
    for WayLook { id, way, on } in looks.ways {
        bar.push(Item::button(
            Slot::Look,
            2,
            tool(
                id,
                way.icon,
                way.tip,
                serde_json::json!({"action":"way","provider":id,"on":!on}),
                way.label,
                on,
            ),
        ));
    }
    if let Some(on) = looks.source {
        bar.push(Item::button(
            Slot::Look,
            2,
            // One wording for both states: a tooltip outlives the click.
            tool(
                "preview-source",
                "code",
                "The file's own text, as written",
                serde_json::json!({"action":"source","on":!on}),
                label::SOURCE,
                on,
            ),
        ));
    }
    if let Some(on) = looks.turn {
        bar.push(Item::button(
            Slot::Look,
            2,
            tool(
                "preview-turn",
                "cube",
                "The model turned in 3D, or its picture",
                serde_json::json!({"action":"turn"}),
                label::TURN,
                on,
            ),
        ));
    }
    if let Some(Whole { on, enabled }) = looks.whole {
        bar.push(Item::button(
            Slot::Look,
            2,
            tool(
                "preview-whole",
                "down",
                "All of this file, past the size a preview keeps to",
                serde_json::json!({"action":"whole","on":!on}),
                label::WHOLE,
                on,
            )
            .enabled(enabled),
        ));
    }
    if let Some(hex) = looks.hex {
        let on = matches!(hex, Hex::Shown);
        let button = tool(
            "preview-hex",
            "grid",
            label::HEX,
            serde_json::json!({"action": if on { "auto" } else { "hex" }}),
            label::HEX,
            on,
        );
        match hex {
            Hex::Row => bar.otherwise.push(button),
            Hex::Shown | Hex::Button => bar.push(Item::button(Slot::Look, 2, button)),
        }
    }
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
    bar.push(
        Item::widget(Slot::Title, 0, move |_, _| {
            // A name never decides which tools fit, nor gives way as a
            // caption comes (lines counted): cut at once, at the room it
            // keeps, three shares of the spare room to a caption's one.
            spare()
                .flex_grow(NAMED)
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
                        .text_body()
                        .text_color(text),
                    name.clone(),
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
        || !bar.otherwise.is_empty()
        || bar
            .items
            .iter()
            .any(|item| !item.shown(mode) && matches!(item.tool, Tool::Button(_)))
}

/// What is about the entry (`Shell::preview_entry_items`, asked as More
/// opened), in its order: a row each, and a row for each submenu where its
/// first item stands, which opens the submenu beside it (`fly_out`), with
/// the first icon among its items, as a context menu's does.
fn entry_rows(popup: &toolbar::Popup, shell: &Shell, cx: &mut Context<Shell>) -> Vec<AnyElement> {
    let mut items: Vec<&MenuItem> = shell.preview.menu_entry.iter().collect();
    items.sort_by_key(|item| item.order);
    let mut parents: Vec<&str> = Vec::new();
    let mut rows = Vec::new();
    for item in items {
        match item.submenu.as_deref() {
            None => rows.push(menu_row(item_button(item), shell, cx)),
            Some(name) if !parents.contains(&name) => {
                parents.push(name);
                rows.push(submenu_row(popup, name, shell, cx));
            }
            Some(_) => {}
        }
    }
    rows
}

/// A menu's item as a popup's button: what it runs, checked while on.
fn item_button(item: &MenuItem) -> Button {
    Button::new(
        format!("preview-entry-{}", item.title),
        item.icon.clone().unwrap_or_default(),
        item.title.clone(),
        item.command.name.to_string(),
    )
    .args(item.command.args.clone())
    .active(item.is_on())
}

/// The row of More's submenu `name`, its first item's icon before it
/// (`toolbar::submenu_row`).
fn submenu_row(
    popup: &toolbar::Popup,
    name: &str,
    shell: &Shell,
    cx: &mut Context<Shell>,
) -> AnyElement {
    let icon = shell
        .preview_sub_items(name)
        .into_iter()
        .find_map(|item| item.icon.clone().filter(|icon| !icon.is_empty()))
        .unwrap_or_default();
    toolbar::submenu_row(
        popup,
        format!("preview-submenu-{name}"),
        name,
        icon,
        name.to_owned().into(),
        None,
        shell,
        cx,
    )
}

/// More's submenu `name`'s rows: its items, called by their title's first
/// part.
pub(crate) fn sub_rows(shell: &Shell, name: &str) -> Vec<toolbar::FlyRow> {
    shell
        .preview_sub_items(name)
        .into_iter()
        .map(|item| {
            let button = item_button(item);
            let title = button.title();
            let label = title.split(" · ").next().unwrap_or(&title).to_owned();
            toolbar::FlyRow::Row(button.label(label))
        })
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

/// The rows of More for what variant `mode` shows no button for: those
/// it dropped, in the bar's order (by slot, then as given, as the row
/// lays them out), or those no variant has a button for (`never`).
fn without_buttons(
    bar: &Bar,
    mode: usize,
    never: bool,
    shell: &Shell,
    cx: &mut Context<Shell>,
) -> Vec<AnyElement> {
    let mut items: Vec<&Item> = bar
        .items
        .iter()
        .filter(|item| !item.shown(mode) && (item.keep == NEVER) == never)
        .collect();
    items.sort_by_key(|item| item.slot);
    items
        .into_iter()
        .filter_map(|item| match &item.tool {
            Tool::Button(button) => Some(menu_row(button.clone(), shell, cx)),
            Tool::Widget(_) => None,
        })
        .collect()
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
    // What stands at the bar's end, after the spacer: find and the whole
    // screen, then how the file is shown.
    let right = |slot: Slot| slot >= Slot::Tools;
    for item in items {
        if right(item.slot) && !last.is_some_and(right) {
            if !filled {
                row = row.child(div().flex_1().min_w_0());
            }
        } else if last.is_some_and(|slot| slot != item.slot) {
            row = row.child(toolbar::separator(theme));
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
    if !last.is_some_and(right) && !filled {
        row = row.child(div().flex_1().min_w_0());
    }
    // More in every bar, so what stands before it keeps its place:
    // disabled where it holds nothing.
    row.child(
        Button::new("preview-more", "more", label::MORE, "ui.preview_menu")
            .active(active == Some(MORE))
            .enabled(has_more(bar, mode))
            // A popup whose own button this variant dropped opens here.
            .anchor_if(anchor, active.is_some() && !anchored)
            .render(shell, cx),
    )
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
                let mut panel = toolbar::menu_panel(popup, "ui.preview_menu", theme, cx)
                    .max_h(sized(480.))
                    .overflow_y_scroll();
                // What this variant has no button for, in the bar's order
                // (as the row lays them out: by slot, then as given), so
                // a narrower panel adds rows where its buttons stood; what
                // this kind never has a button for (`Item::menu_only`);
                // the kind's settings; what is about the entry; another
                // way of looking. Groups apart, each there when it holds
                // a row.
                let groups: [Vec<AnyElement>; 5] = [
                    without_buttons(bar, mode, false, shell, cx),
                    without_buttons(bar, mode, true, shell, cx),
                    bar.settings
                        .as_ref()
                        .map(|settings| settings(shell, theme, cx))
                        .unwrap_or_default(),
                    if bar.always {
                        entry_rows(popup, shell, cx)
                    } else {
                        Vec::new()
                    },
                    bar.otherwise
                        .iter()
                        .map(|button| menu_row(button.clone(), shell, cx))
                        .collect(),
                ];
                for (at, group) in groups
                    .into_iter()
                    .filter(|group| !group.is_empty())
                    .enumerate()
                {
                    if at > 0 {
                        panel = panel.child(separator(theme));
                    }
                    panel = panel.children(group);
                }
                // Resting on a row closes the submenu open, but on its own.
                panel
                    .on_mouse_move(cx.listener(|this, _, window, cx| {
                        this.popup_sub_rest(None, window, cx);
                    }))
                    .into_any_element()
            })
            .collect();
        root = root.child(anchor.popup_for(&chosen, panels));
        // Above More, beside the row that opens it.
        if let Some(name) = popup.sub.open.clone() {
            let panel = toolbar::fly_out(popup, &name, shell.submenu_rows(&name), shell, theme, cx);
            root = root.child(popup.sub.row.popup_beside(4, crate::menu::SUB_INSET, panel));
        }
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
    /// Where its popup was last drawn (`find_popup`): what it hides of
    /// what the preview shows under it.
    pub drawn: Rc<std::cell::Cell<Option<gpui::Bounds<gpui::Pixels>>>>,
    /// What the last thing asked of it came to, and whether that is a
    /// failure (`say`), until its text changes.
    said: Rc<std::cell::RefCell<Option<(String, bool)>>>,
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
        let said: Rc<std::cell::RefCell<Option<(String, bool)>>> = Rc::default();
        let heard = Rc::clone(&said);
        let subscription = cx.subscribe_in(
            &input,
            window,
            move |this, _, event: &InputEvent, window, cx| {
                let (command, args) = match event {
                    InputEvent::Submit => split(submit),
                    InputEvent::Cancel => split(cancel),
                    InputEvent::Changed => {
                        // What was said was about the text before.
                        heard.take();
                        return cx.notify();
                    }
                };
                this.run(command, args, window, cx);
            },
        );
        Field {
            input,
            drawn: Rc::default(),
            said,
            _subscription: subscription,
        }
    }

    pub fn text(&self, cx: &gpui::App) -> String {
        self.input.read(cx).text().to_owned()
    }

    /// Say in its popup what the last thing asked came to instead of what
    /// was looked for (no match, why it was not looked for, a failure):
    /// the answer stays where the question was (shell.md, "Feedback").
    pub fn say(&self, text: impl Into<String>, error: bool) {
        *self.said.borrow_mut() = Some((text.into(), error));
    }

    /// Asked again: what was said of the last time goes.
    pub fn hush(&self) {
        self.said.take();
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
/// structured preview: the field, a line saying where the search stands
/// (`status`) or what the last find came to (`Field::say`), and the
/// buttons that step through the matches and close it. `context` names
/// its keys (`keymap::PREVIEW_FIND`). The field is told where it was drawn
/// (`Field::drawn`).
pub fn find_popup(
    field: &Field,
    context: &'static str,
    status: String,
    buttons: Vec<Button>,
    shell: &Shell,
    theme: &Theme,
    cx: &mut Context<Shell>,
) -> AnyElement {
    let line = match field.said.borrow().clone() {
        // Announced as a toast is: a failure at once, the rest after what
        // is being said (accessibility.md).
        Some((said, error)) => div()
            .id("preview-find-status")
            .role(if error {
                gpui::Role::Alert
            } else {
                gpui::Role::Status
            })
            .aria_live(if error {
                gpui::Live::Assertive
            } else {
                gpui::Live::Polite
            })
            .when(shell.a11y, |d| d.aria_label(said.clone()))
            .text_color(if error { theme.error } else { theme.text })
            .child(said),
        None => div()
            .id("preview-find-status")
            .text_color(theme.text_muted)
            .child(status),
    };
    div()
        .tab_group()
        .key_context(context)
        .w(sized(280.))
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
                .text_body()
                .child(line.flex_1().min_w_0())
                .children(buttons.into_iter().map(|button| button.render(shell, cx))),
        )
        .child({
            let drawn = Rc::clone(&field.drawn);
            gpui::canvas(
                move |bounds, _, _| drawn.set(Some(bounds)),
                |_, (), _, _| {},
            )
            .absolute()
            .top_0()
            .left_0()
            .size_full()
        })
        .into_any_element()
}
