//! Shared command-backed toolbar buttons, bundled symbolic icons and popovers.
use crate::sizes::TextSizes;
use crate::sizes::sized;
use crate::theme::Chosen;
use crate::{keymap::RunCommand, shell::Shell, theme::Theme};
use gpui::{
    AnyView, App, Context, Div, FocusHandle, Render, ScrollHandle, SharedString, Stateful, Window,
    div, prelude::*, px, svg,
};
use skwir_core::workspace::PaneId;

pub fn asset(path: &str) -> Option<&'static [u8]> {
    macro_rules! assets {
        ($($name:literal),* $(,)?) => { match path {
            $(concat!("toolbar/", $name, ".svg") => Some(include_bytes!(concat!("../assets/toolbar/", $name, ".svg")).as_slice()),)*
            _ => None,
        }};
    }
    assets!(
        "star",
        "down",
        "eye-off",
        "star-filled",
        "star-half",
        "comment",
        "back",
        "forward",
        "up",
        "list",
        "tree",
        "busy",
        "grid",
        "columns",
        "board",
        "preview",
        "preview-bottom",
        "sidebar",
        "split",
        "split-rows",
        "usage",
        "layers",
        "cube",
        "perspective",
        "smooth",
        "map",
        "location-minus",
        "spread",
        "grip",
        "calendar",
        "search",
        "filter",
        "settings",
        "more",
        "contents",
        "continuous",
        "left",
        "right",
        "plus",
        "minus",
        "unfold",
        "fold",
        "fit",
        "width",
        "copy",
        "open",
        "close",
        "chevron",
        "chevron-up",
        "eye",
        "move",
        "refresh",
        "play",
        "pause",
        "volume",
        "muted",
        "fullscreen",
        "revert",
        "redo",
        "power",
        "check",
        "warning",
        "link",
        "plugin",
        "keyboard",
        "appearance",
        "sliders",
        "network",
        "external",
        "chevron-right",
        "sort",
        "folder-plus",
        "file-plus",
        "command",
        "rename",
        "cut",
        "paste",
        "trash",
        "tag",
        "shelf",
        "recent",
        "eject",
        "home",
        "info",
        "crop",
        "code",
        "skip-next",
        "skip-previous",
        "pin",
        "play-next",
        "shuffle",
        "reading",
        "bookmark",
        "bookmark-filled",
        "folder",
        "tab-new",
        "window-new",
        "terminal",
        "lock",
        "archive",
        "printer",
    )
}

/// The widest a tooltip grows at 100 %: a long name (`elements::clip`)
/// wraps. It grows with the text (ADR-117), never wider than the window.
const TIP_MAX: f32 = 480.;

/// A tooltip: a view of its own, drawn outside what it is the tooltip of,
/// so it says its font itself (the theme's, ADR-073).
struct Tip {
    text: SharedString,
}
impl Render for Tip {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.global::<Theme>();
        // Laid out on its own, with no width to be a share of: the
        // window's, less a margin each side.
        let most = theme
            .sizes
            .scale(crate::sizes::Part::Interface)
            .f(TIP_MAX)
            .min(f32::from(window.viewport_size().width) - 16.)
            .max(1.);
        div()
            .font_family(theme.font.clone())
            .text_body()
            .text_color(theme.text)
            .bg(theme.surface)
            .border_1()
            .border_color(theme.border)
            .rounded_md()
            .shadow_sm()
            .max_w(px(most))
            .px_2()
            .py_1()
            .child(self.text.clone())
    }
}
pub(crate) fn tooltip(text: SharedString, cx: &mut App) -> AnyView {
    cx.new(|_| Tip { text }).into()
}

#[derive(Clone)]
#[allow(
    clippy::struct_excessive_bools,
    reason = "independent states of one button: on, usable, keeping its popup open, given now by Automatic"
)]
pub struct Button {
    id: SharedString,
    icon: SharedString,
    title: SharedString,
    command: SharedString,
    args: serde_json::Value,
    label: Option<SharedString>,
    anchor: Option<crate::elements::popover::PopoverAnchor>,
    pub active: bool,
    pub enabled: bool,
    shape: Shape,
    tint: Option<Tint>,
    /// What a long press runs instead of the click (`HOLD`).
    hold: Option<Box<(SharedString, serde_json::Value)>>,
    /// A popup's row that leaves the popup open when pressed (`keeps_open`).
    keeps_open: bool,
    /// What an automatic value does now, while it is the one chosen: a
    /// dashed outline (`now`).
    now: bool,
    /// A popup's row that opens a submenu beside it (`opens_beside`):
    /// whether that submenu is open.
    beside: Option<bool>,
}

/// A tag's colors on a button (a narrowing by that tag), in place of
/// `active`'s: filled with the tag's color, as its chips are, or
/// `hollow`, its outline only, for the files without it.
#[derive(Clone, Copy)]
pub struct Tint {
    /// An index into `Theme::tags`, eight of them.
    color: u8,
    hollow: bool,
}

impl Tint {
    /// `tag`'s, in the color the user's choices give it.
    pub fn of(tag: &skwir_core::metadata::TagRef, colors: &crate::tags::TagColors) -> Option<Self> {
        Some(Tint {
            color: u8::try_from(colors.of(&tag.name)).ok()?,
            hollow: tag.lacks,
        })
    }
}

/// How a button stands among its neighbours.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Shape {
    /// In a toolbar: its icon, and its label if it has one.
    Tool,
    /// Opens a menu: a small chevron after the icon says so.
    Dropdown,
    /// A row of a popup menu: full width, left-aligned, with an icon slot
    /// that stays when the icon is empty so labels line up.
    MenuRow,
    /// One value of a setting on a popup's row (`MenuPart::Choice`):
    /// small, its icon or a word, filled when chosen.
    Segment,
    /// One value among a popup's tiles (`MenuPart::Tiles`): its icon
    /// over its name, filled when chosen.
    Tile,
    /// A group's name in a popup menu that turns the group on or off
    /// (`MenuPart::Heading`): the name in a heading's words, a switch at
    /// its end, the whole of it pressed.
    Switch,
}
impl Button {
    pub fn new(
        id: impl Into<SharedString>,
        icon: impl Into<SharedString>,
        title: impl Into<SharedString>,
        command: impl Into<SharedString>,
    ) -> Self {
        Self {
            id: id.into(),
            icon: icon.into(),
            title: title.into(),
            command: command.into(),
            args: serde_json::json!({}),
            label: None,
            anchor: None,
            active: false,
            enabled: true,
            tint: None,
            shape: Shape::Tool,
            hold: None,
            keeps_open: false,
            now: false,
            beside: None,
        }
    }
    pub fn title(&self) -> SharedString {
        self.title.clone()
    }
    pub fn args(mut self, args: serde_json::Value) -> Self {
        self.args = args;
        self
    }
    pub fn label(mut self, label: impl Into<SharedString>) -> Self {
        self.label = Some(label.into());
        self
    }
    pub fn menu(mut self) -> Self {
        self.shape = Shape::MenuRow;
        self
    }
    /// One value of a setting on a popup's row (`MenuPart::Choice`); the
    /// popup stays open once it is chosen, for another.
    pub fn segment(mut self) -> Self {
        self.shape = Shape::Segment;
        self.keeps_open = true;
        self
    }
    /// One value among a popup's tiles (`MenuPart::Tiles`); the popup
    /// stays open once it is chosen.
    pub fn tile(mut self) -> Self {
        self.shape = Shape::Tile;
        self.keeps_open = true;
        self
    }
    /// A menu's heading that turns its group on or off, `active` while on
    /// (`MenuPart::Heading`); the popup stays open once it is pressed, the
    /// group's rows following.
    pub fn switch(mut self) -> Self {
        self.shape = Shape::Switch;
        self.keeps_open = true;
        self
    }
    /// What the automatic value, chosen, does now: this value, outlined.
    pub fn now(mut self, now: bool) -> Self {
        self.now = now;
        self
    }
    /// A popup's row that opens a submenu beside it, as a context menu's
    /// does (`Popup::sub`): a chevron at its end, expanded while `open`,
    /// the popup kept open when it is pressed, and Right opens it as Enter
    /// does (`SUBMENU`, `ui.toolbar_submenu`'s `here`).
    pub fn opens_beside(mut self, open: bool) -> Self {
        self.shape = Shape::MenuRow;
        self.keeps_open = true;
        self.beside = Some(open);
        self
    }
    /// Draw the chevron of a button that opens a menu.
    pub fn dropdown(mut self) -> Self {
        self.shape = Shape::Dropdown;
        self
    }
    /// The button is about a tag: drawn in its colors (`Tint`).
    pub fn tint(mut self, tint: Option<Tint>) -> Self {
        self.tint = tint;
        self
    }

    /// Whether it is drawn in a tag's colors, which what lies on it
    /// follows (a pill's ×).
    pub fn tinted(&self) -> bool {
        self.tint.is_some()
    }

    pub fn active(mut self, active: bool) -> Self {
        self.active = active;
        self
    }
    /// A press held `HOLD` runs `command` with `args` instead of the
    /// click, as a browser's Back and Forward list their history.
    pub fn on_hold(mut self, command: impl Into<SharedString>, args: serde_json::Value) -> Self {
        self.hold = Some(Box::new((command.into(), args)));
        self
    }
    pub fn enabled(mut self, enabled: bool) -> Self {
        self.enabled = enabled;
        self
    }
    /// A popup's row that switches one of several choices the popup
    /// shows (which views have a button): pressed, the popup stays open,
    /// its row keeping the keyboard, so the next is a press away. Any
    /// other button closes the popup open.
    pub fn keeps_open(mut self) -> Self {
        self.keeps_open = true;
        self
    }
    pub fn anchor_if(
        mut self,
        anchor: &crate::elements::popover::PopoverAnchor,
        active: bool,
    ) -> Self {
        if active {
            self.anchor = Some(anchor.clone());
        }
        self
    }
    pub fn render(self, shell: &Shell, cx: &mut Context<Shell>) -> Stateful<Div> {
        let theme = cx.global::<Theme>().clone();
        let pane = shell.pane;
        let command = self.command.clone();
        let args = self.args.clone();
        let enabled = self.enabled;
        let keeps_open = self.keeps_open;
        let menu = self.shape == Shape::MenuRow;
        let (segment, tiled) = (self.shape == Shape::Segment, self.shape == Shape::Tile);
        let switch = self.shape == Shape::Switch;
        // A value of a setting, filled when chosen rather than highlighted.
        let value = segment || tiled;
        let now = self.now && !self.active;
        // The shortcut comes from the keymap, never from the title: it
        // follows what is bound, and goes when its plugin does.
        let keys = shell.keys_for(&command, &args);
        // To assistive technology: named by the words it shows, else by its
        // title, which then describes it.
        let said = shell.a11y.then(|| {
            let described = self.label.is_some().then(|| self.title.clone());
            let named = self.label.clone().unwrap_or_else(|| self.title.clone());
            (named, described, keys.clone())
        });
        let menu_keys = keys.clone().filter(|_| menu);
        let title: SharedString = match keys {
            Some(keys) => format!("{} · {keys}", self.title).into(),
            None => self.title,
        };
        let (active, dropdown, open) = (
            self.active,
            self.shape == Shape::Dropdown,
            self.anchor.is_some(),
        );
        let beside = self.beside;
        // A label cut short says itself in the tooltip, before the title.
        let (label, clip) = self
            .label
            .map(crate::elements::clip::text)
            .map_or((None, None), |(label, clip)| (Some(label), Some(clip)));
        let id = self.id.clone();
        let clicked = id.clone();
        // A tag's colors: the fill and the edge, then under the pointer.
        let tint = self.tint.map(|tint| {
            let color = usize::from(tint.color);
            let (tone, fill) = (theme.tag(color), theme.tag_fill(color));
            if tint.hollow {
                (gpui::transparent_black(), tone, fill, tone)
            } else {
                (fill, gpui::transparent_black(), fill, tone)
            }
        });
        let revealed = cx
            .try_global::<RevealRow>()
            .is_some_and(|reveal| reveal.row.as_ref() == Some(&id));
        // A glyph of the button (its icon, a dropdown's chevron): a value's
        // accent on `term` while chosen, else an icon's color, drawn in the
        // selection's ink while the button is on it (`Chosen`).
        let on_term = self.active && value && tint.is_none();
        let glyph = |svg: gpui::Svg| {
            svg.text_color(if on_term {
                theme.accent_text
            } else {
                theme.text_muted
            })
        };
        let button = div()
            .id(self.id)
            // A setting's value is one of a radio group (`MenuPart::Choice`,
            // `MenuPart::Tiles`), checked when chosen (ADR-099).
            .role(if menu {
                gpui::Role::MenuItem
            } else if switch {
                gpui::Role::Switch
            } else if value {
                gpui::Role::RadioButton
            } else {
                gpui::Role::Button
            })
            .when_some(said, |d, (named, described, keys)| {
                d.aria_label(named)
                    .when_some(described, StatefulInteractiveElement::aria_description)
                    .when_some(keys, StatefulInteractiveElement::aria_keyshortcuts)
                    .aria_disabled(!enabled)
                    // What it stands for is on: the view shown, the
                    // history's place, a narrowing applied.
                    .when(active || value, |d| {
                        if menu {
                            d.aria_selected(true)
                        } else {
                            d.aria_toggled(active.into())
                        }
                    })
                    // A switch says it is off too.
                    .when(switch && !active, |d| d.aria_toggled(false.into()))
                    .when(dropdown, |d| d.aria_expanded(open))
                    .when_some(beside, StatefulInteractiveElement::aria_expanded)
            })
            .relative()
            .h(sized(if menu || switch { MENU_ROW } else { 30. }))
            // Its icon keeps its size and its padding grows with the text:
            // an icon alone is as wide as that, not as tall as the row, so a
            // bar's parts take no more room than they hold (ADR-117).
            .min_w(px(30.))
            .flex_none()
            .px(sized(6.))
            .flex()
            .items_center()
            .when(!menu && !switch, gpui::Styled::justify_center)
            .gap(sized(6.))
            .rounded_md()
            .border_1()
            .border_color(gpui::transparent_black())
            .text_body()
            .whitespace_nowrap()
            .text_color(theme.text_muted)
            // A popup's row, as a context menu's (`menu::render`): the
            // window's text, the row under the pointer or the keyboard
            // highlighted, what is on said by a check, not a fill.
            .when(menu, |d| {
                d.w_full().px_2().gap_2().text_body().text_color(theme.text)
            })
            // A heading's words, the switch at the row's end.
            .when(switch, |d| {
                d.w_full()
                    .justify_between()
                    .px_2()
                    .gap_4()
                    .text_small()
                    .text_color(theme.text_muted)
            })
            .when(segment, |d| d.h(sized(22.)).min_w(px(26.)).gap(sized(4.)))
            .when(tiled, |d| {
                d.h(sized(52.))
                    .w_full()
                    .flex_col()
                    .gap(sized(5.))
                    .text_color(theme.text)
            })
            // Keep the handle alive when navigation disables this button.
            // Dropping the focused handle would also lose ancestor shortcuts.
            .focusable()
            .tab_index(0)
            .tab_stop(enabled)
            .when(self.active && !menu && !value && !switch, |d| {
                d.chosen(&theme)
            })
            .when(self.active && value, |d| {
                d.bg(theme.term).text_color(theme.accent_text)
            })
            .when(now, |d| d.border_dashed().border_color(theme.accent))
            .when_some(tint, |d, (fill, edge, _, _)| {
                d.bg(fill).border_color(edge).text_color(theme.text)
            })
            .when(enabled && !menu && !switch, |d| {
                d.cursor_pointer()
                    .when(!(value && active), |d| {
                        d.hover(|s| match tint {
                            Some((_, _, fill, edge)) => s.bg(fill).border_color(edge),
                            None => s.bg(theme.hover).text_color(theme.text),
                        })
                    })
                    .focus(|s| s.border_color(theme.accent))
            })
            .when(enabled && (menu || switch), |d| {
                d.cursor_pointer()
                    .hover(|s| s.chosen(&theme))
                    .focus(|s| s.chosen(&theme))
            })
            .when(!enabled, |d| d.opacity(0.35))
            .tooltip(move |_, cx| {
                let tip = clip
                    .as_ref()
                    .and_then(|clip| clip.says(Some(&title)))
                    .unwrap_or_else(|| title.clone());
                tooltip(tip, cx)
            })
            // A menu row without an icon keeps its slot, so labels line up,
            // and says it is on with a check there.
            .when(menu && self.icon.is_empty(), |d| {
                d.child(if self.active {
                    menu_glyph("check", &theme).size(px(16.)).into_any_element()
                } else {
                    div().size(px(16.)).flex_none().into_any_element()
                })
            })
            .when(!self.icon.is_empty(), |d| {
                d.child(if menu {
                    menu_glyph(&self.icon, &theme).size(px(16.))
                } else {
                    glyph(svg().path(format!("toolbar/{}.svg", self.icon)))
                        .size(px(if tiled {
                            18.
                        } else if segment {
                            15.
                        } else {
                            16.
                        }))
                        .flex_none()
                })
            })
            // A label too long for a menu keeps both ends, as names do.
            .children(label.map(|label| {
                div()
                    .when(menu || switch, gpui::Styled::flex_1)
                    .min_w_0()
                    .overflow_hidden()
                    .text_ellipsis_middle()
                    .child(label)
            }))
            // A menu row says its keys, as a context menu's does.
            .children(menu_keys.map(|keys| {
                div()
                    .flex_none()
                    .pl_6()
                    .text_small()
                    .text_color(theme.text_muted)
                    .child(keys)
            }))
            // On, with an icon of its own: the check after the words.
            .when(
                menu && self.active && !self.icon.is_empty() && self.icon != "check",
                |d| d.child(menu_glyph("check", &theme).size(px(14.))),
            )
            .when(switch, |d| {
                d.child(crate::controls::pill(self.active, &theme))
            })
            .when(self.shape == Shape::Dropdown, |d| {
                d.gap(sized(2.)).child(
                    glyph(svg().path("toolbar/chevron.svg"))
                        .size(px(10.))
                        .flex_none(),
                )
            })
            // A submenu's row, as a context menu's (`menu::row_element`).
            .when(beside.is_some(), |d| {
                d.child(div().flex_1()).child(
                    svg()
                        .path("toolbar/chevron-right.svg")
                        .size(px(13.))
                        .flex_none()
                        .text_color(theme.text_muted),
                )
            })
            .children(self.anchor.map(|anchor| anchor.marker()))
            .when(revealed, |d| d.child(reveal_marker()))
            // A long press: `HOLD` after the press, what it holds runs, and
            // the release that follows clicks nothing.
            .when_some(self.hold.filter(|_| enabled), |d, hold| {
                let pressed = clicked.clone();
                d.on_mouse_down(
                    gpui::MouseButton::Left,
                    cx.listener(move |this, _, window, cx| {
                        let (id, hold) = (pressed.clone(), hold.clone());
                        let timer = cx.spawn_in(window, async move |this, cx| {
                            cx.background_executor().timer(HOLD).await;
                            let _ = this.update_in(cx, |this, window, cx| {
                                let Some(press) = this.held.as_mut().filter(|press| press.id == id)
                                else {
                                    return;
                                };
                                press.fired = true;
                                let (command, args) = *hold;
                                invoke(this, pane, &command, args, true, window, cx);
                            });
                        });
                        this.held = Some(Held {
                            id: pressed.clone(),
                            fired: false,
                            _timer: timer,
                        });
                    }),
                )
                // Let go elsewhere: no long press.
                .on_mouse_up_out(
                    gpui::MouseButton::Left,
                    cx.listener(|this, _, _, _| this.held = None),
                )
            });
        // Its id says which row of a popup the keyboard reached
        // (`Shell::reveal_focused_row`).
        crate::modal::pressed_with(
            button,
            |keys| {
                keys.set(ROW, id);
                // Left and Right walk a setting's values (`keymap`).
                if value {
                    keys.add(CHOICE);
                }
                // Right opens a submenu beside its row.
                if beside.is_some() {
                    keys.add(SUBMENU);
                }
            },
            move |this, window, cx| {
                // The release of a long press: what it held ran instead.
                if this
                    .held
                    .take()
                    .is_some_and(|press| press.id == clicked && press.fired)
                {
                    return;
                }
                if enabled {
                    let close = !keeps_open;
                    invoke(this, pane, &command, args.clone(), close, window, cx);
                }
            },
            cx,
        )
        .control()
    }
}

/// The height of a menu's row: a popup's (`Button::menu`) and a context
/// menu's (`menu::render`) alike.
pub(crate) const MENU_ROW: f32 = 28.;

/// A bundled icon on a menu's row, muted: drawn in the selection's ink
/// while the row is highlighted (`Chosen`).
pub(crate) fn menu_glyph(icon: &str, theme: &Theme) -> gpui::Svg {
    svg()
        .path(format!("toolbar/{icon}.svg"))
        .flex_none()
        .text_color(theme.text_muted)
}

/// How long a press is held before it is a long press: GTK's default
/// (`gtk-long-press-time`).
const HOLD: std::time::Duration = std::time::Duration::from_millis(500);

/// A press on a button that does something else when held (`on_hold`),
/// the window's one: its timer runs the hold unless the press ends first.
pub(crate) struct Held {
    id: SharedString,
    /// The hold ran: the release clicks nothing.
    fired: bool,
    _timer: gpui::Task<()>,
}

/// Run a button's command on its pane, closing the popup open unless the
/// button keeps it open (`close`, `Button::keeps_open`).
fn invoke(
    shell: &mut Shell,
    pane: PaneId,
    command: &str,
    mut args: serde_json::Value,
    close: bool,
    window: &mut Window,
    cx: &mut Context<Shell>,
) {
    if shell.app.workspace.read().pane(pane).is_none() {
        return;
    }
    shell.activate_pane(pane, cx);
    // What a button runs closes the popup open, as a press outside it
    // does: a menu's row closes its menu, and the popup's own button,
    // pressed again, leaves it closed (`Shell::toggle_popup`).
    if close && let Some(was) = shell.close_popup(window, cx) {
        shell.dismissed = Some((shell.presses, was));
    }
    shell.note_pointer_use(command, &args, cx);
    // A command acts on the button's pane, unless the button names the
    // pane it acts on (a location's tool for the other pane's folder), or
    // the command takes none (a place's Eject): its arguments refuse what
    // they do not declare.
    if !command.starts_with("ui.")
        && args.get("pane").is_none()
        && shell
            .app
            .commands
            .get(command)
            .is_some_and(|spec| spec.accepts("pane"))
    {
        args["pane"] = serde_json::json!(pane);
    }
    shell.run(command, args, window, cx);
}

impl Shell {
    /// Close the popup open, whichever it is (More or the View menu, a
    /// menu of places, a search pill's choices, a preview's menu): there is
    /// one at most. Returns what it was, as its toggle names it. A menu of
    /// places still being read is no longer wanted (`nav_asked`).
    pub(crate) fn close_popup(
        &mut self,
        window: &mut Window,
        cx: &mut gpui::App,
    ) -> Option<String> {
        self.nav_asked += 1;
        let mut was = None;
        if let Some(popup) = self.toolbar_menu.take() {
            was = Some(popup.kind.to_owned());
            popup.restore(window, cx);
        }
        if let Some(what) = self.close_nav_menu(window, cx) {
            was = Some(what.key());
        }
        if let Some(key) = self.close_search_chip(window, cx) {
            was = Some(key);
        }
        if let Some(popup) = self.preview.menu.take() {
            was = Some(format!("preview {}", popup.kind));
            popup.restore(window, cx);
        }
        was
    }

    /// Close the popup open and what is on its way to one (a menu of
    /// places being read, a note read from another chapter), as a dialog
    /// that holds the window's commands appears: Settings, the overview,
    /// About (`Shell::takes_now`). A popup left under it runs its close at
    /// every press outside it, a press in the dialog included, which the
    /// dialog refuses (the popup stays over it) or closes for. Before the
    /// dialog takes the keyboard: what had it before the popup has it then.
    pub(crate) fn close_popups_for_dialog(&mut self, window: &mut Window, cx: &mut gpui::App) {
        self.close_popup(window, cx);
        self.preview.inspection.notes.close(window, cx);
    }

    /// Every popup's toggle (`args`: `close`, and `pointer` when a press
    /// outside closes it), before opening the one named `key`: the popup
    /// open closes, and this one opens unless asked to close, unless it
    /// was the one open (a second press closes it), and unless the press
    /// that begins this click just closed it. One popup at a time, the
    /// same way for all.
    pub(crate) fn toggle_popup(
        &mut self,
        key: &str,
        args: &serde_json::Value,
        window: &mut Window,
        cx: &mut gpui::App,
    ) -> bool {
        let was = self.close_popup(window, cx);
        if args.get("close").and_then(serde_json::Value::as_bool) == Some(true) {
            if let Some(was) = was {
                self.note_dismissed(args, was);
            }
            return false;
        }
        was.as_deref() != Some(key) && !self.just_dismissed(key)
    }

    /// The keyboard moved to another row of the open popup (the arrows,
    /// Tab, Home, End): that row, once drawn, scrolls into view
    /// (`RevealRow`). Which row it is is read from the focused element's
    /// key context, never from layout.
    pub(crate) fn reveal_focused_row(&mut self, window: &Window, cx: &mut Context<Self>) {
        let stack = window.context_stack();
        if !stack.iter().any(|context| context.contains("ToolbarPopup")) {
            return;
        }
        let Some(row) = stack
            .iter()
            .rev()
            .find(|context| context.contains(crate::modal::BUTTON))
            .and_then(|context| context.get(ROW).cloned())
        else {
            return;
        };
        cx.set_global(RevealRow {
            row: Some(row),
            scroll: None,
        });
        cx.notify();
    }

    /// `ui.toolbar_first`, `ui.toolbar_last`: the keyboard to the first
    /// or the last row of the popup it is in. GPUI knows only the stop
    /// before and the one after: walk them while the keyboard stays in the
    /// popup (`ToolbarPopup`), and stay at the last one that was.
    pub(crate) fn toolbar_popup_end(&mut self, last: bool, window: &mut Window, cx: &mut App) {
        let in_popup = |window: &Window| {
            window
                .context_stack()
                .iter()
                .any(|context| context.contains("ToolbarPopup"))
        };
        if !in_popup(window) {
            return;
        }
        // More than a popup's rows: a folder's menu has `FOLDER_ROWS` and one.
        for _ in 0..1000 {
            let Some(at) = window.focused(cx) else {
                return;
            };
            if last {
                window.focus_next(cx);
            } else {
                window.focus_prev(cx);
            }
            if !in_popup(window) || window.focused(cx).as_ref() == Some(&at) {
                window.focus(&at, cx);
                return;
            }
        }
    }

    /// A popup's toggle, closing it: when a press outside closed it
    /// (`pointer` in `args`), note which (`what`) and with which press.
    pub(crate) fn note_dismissed(&mut self, args: &serde_json::Value, what: String) {
        if args.get("pointer").and_then(serde_json::Value::as_bool) == Some(true) {
            self.dismissed = Some((self.presses, what));
        }
    }

    /// A popup's toggle, opening it: whether the press this click began
    /// has just closed that same popup. The press lands outside the popup,
    /// on the button that opened it, and closes it first; the click that
    /// follows then leaves it closed, as a second click on a menu button
    /// does everywhere.
    pub(crate) fn just_dismissed(&mut self, what: &str) -> bool {
        let presses = self.presses;
        self.dismissed
            .take()
            .is_some_and(|(press, closed)| press == presses && closed == what)
    }
}

pub fn separator(theme: &Theme) -> Div {
    div()
        .w(px(1.))
        .h(sized(18.))
        .mx(px(5.))
        .flex_none()
        .bg(theme.border)
}
/// How tall a toolbar's row is.
pub const HEIGHT: f32 = 42.;

/// A toolbar's row, drawn on the chrome its area paints (a preview's
/// bar on the panel's, Peek's or the whole screen's: `preview::controls`),
/// or on one its caller gives it.
pub fn row(theme: &Theme) -> Div {
    div()
        .tab_group()
        .relative()
        .w_full()
        .h(sized(HEIGHT))
        .flex()
        .items_center()
        .gap(sized(2.))
        .px(sized(6.))
        .border_b_1()
        .border_color(theme.border)
}

pub struct Popup {
    pub kind: &'static str,
    pub focus: FocusHandle,
    previous: crate::modal::FocusReturn,
    /// How far its rows are scrolled, for the keyboard to bring the row
    /// it reaches into view (`RevealRow`).
    scroll: ScrollHandle,
    /// The submenu open beside one of its rows, if any.
    pub sub: Submenu,
}

/// A popup's submenu, open beside its row (`Button::opens_beside`) as a
/// context menu's is (docs/invariants/shell.md, "Context menu"): one at a
/// time, holding no submenu; the pointer opens it after resting
/// `menu::REST` on its row, or at once with a press, and resting on
/// another row closes it. Its rows have the keyboard's focus as the
/// popup's do, walked by `ui.toolbar_submenu`: drawn beside the popup,
/// they are not where gpui's tab order would reach them.
#[derive(Default)]
pub struct Submenu {
    /// The submenu shown, by its name.
    pub open: Option<SharedString>,
    /// Where its row stands, and where it is drawn, both measured in
    /// prepaint: a press there is no press outside the popup.
    pub row: crate::elements::popover::PopoverAnchor,
    pub area: crate::elements::popover::PopoverAnchor,
    /// Its rows' focus, in their order.
    pub rows: Vec<FocusHandle>,
    /// The focus of each submenu's row, which Left and Escape go back to.
    pub parents: Vec<(SharedString, FocusHandle)>,
    /// The pointer resting on a row: the submenu it opens, or the one open
    /// it closes, once it has rested.
    pub rest: Option<gpui::Task<()>>,
}

impl Submenu {
    /// The focus of the row that opens `name`.
    pub fn parent(&self, name: &str) -> Option<&FocusHandle> {
        self.parents
            .iter()
            .find(|(parent, _)| parent.as_ref() == name)
            .map(|(_, focus)| focus)
    }

    /// Show `name` beside its row, with `rows` rows: the keyboard stays
    /// where it is.
    pub fn show(&mut self, name: SharedString, rows: usize, cx: &mut gpui::App) {
        self.rest = None;
        if self.open.as_ref() == Some(&name) {
            return;
        }
        self.open = Some(name);
        self.rows = (0..rows).map(|_| cx.focus_handle()).collect();
        self.area = crate::elements::popover::PopoverAnchor::default();
    }

    /// Close the one open, if any.
    pub fn hide(&mut self) {
        self.rest = None;
        self.open = None;
        self.rows.clear();
        self.area = crate::elements::popover::PopoverAnchor::default();
    }
}
/// A row of a popup's submenu (`fly_out`): a button, or a line between
/// its groups.
// A submenu's dozen rows, made for one frame and drawn at once: boxing
// each button would allocate more than the lines' room costs.
#[allow(clippy::large_enum_variant)]
pub enum FlyRow {
    Row(Button),
    Line,
}

/// How many of `rows` the keyboard reaches.
pub fn row_count(rows: &[FlyRow]) -> usize {
    rows.iter()
        .filter(|row| matches!(row, FlyRow::Row(_)))
        .count()
}

impl Shell {
    /// The open popup that may hold a submenu beside a row: the preview's
    /// More, else a toolbar menu (the View menu's Sort by and Group by).
    pub(crate) fn submenu_popup(&mut self) -> Option<&mut Popup> {
        if self
            .preview
            .menu
            .as_ref()
            .is_some_and(|popup| popup.kind == "preview")
        {
            return self.preview.menu.as_mut();
        }
        self.toolbar_menu.as_mut()
    }

    /// The rows of the open popup's submenu `name`.
    pub(crate) fn submenu_rows(&self, name: &str) -> Vec<FlyRow> {
        if self
            .preview
            .menu
            .as_ref()
            .is_some_and(|popup| popup.kind == "preview")
        {
            return crate::preview_bar::sub_rows(self, name);
        }
        match &self.toolbar_menu {
            Some(popup) if popup.kind == crate::address::VIEW_OPTIONS => {
                crate::address::view_submenu(self, name)
            }
            _ => Vec::new(),
        }
    }
}

/// The row of `popup`'s submenu `name`, called `label`, `icon` before it:
/// pressed (Enter, Space, Right), it opens the submenu with the keyboard
/// on its first row; the pointer resting on it opens it too, and resting
/// on another row of the popup closes it (`Shell::popup_sub_rest`). Lit
/// while open, as the context menu's.
pub fn submenu_row(
    popup: &Popup,
    id: String,
    name: &str,
    icon: impl Into<SharedString>,
    label: SharedString,
    shell: &Shell,
    cx: &mut Context<Shell>,
) -> gpui::AnyElement {
    let theme = cx.global::<Theme>().clone();
    let open = popup.sub.open.as_deref() == Some(name);
    let mut row = Button::new(id, icon, label.to_string(), "ui.toolbar_submenu")
        .args(serde_json::json!({ "open": name }))
        .label(label)
        .opens_beside(open)
        .anchor_if(&popup.sub.row, true)
        .render(shell, cx);
    if let Some(focus) = popup.sub.parent(name) {
        row = row.track_focus(focus);
    }
    let rests: SharedString = name.to_owned().into();
    row.when(open, |row| row.chosen(&theme))
        .on_mouse_move(cx.listener(move |this, _, window, cx| {
            this.popup_sub_rest(Some(rests.clone()), window, cx);
            cx.stop_propagation();
        }))
        .into_any_element()
}

/// `popup`'s submenu `name`, beside its row: `rows`, each button one the
/// keyboard reaches (`Submenu::rows`, walked by `ui.toolbar_submenu`), a
/// menu of its own to assistive technology, named by its row.
pub fn fly_out(
    popup: &Popup,
    name: &str,
    rows: Vec<FlyRow>,
    shell: &Shell,
    theme: &Theme,
    cx: &mut Context<Shell>,
) -> gpui::AnyElement {
    let mut panel = div()
        .id("popup-submenu-rows")
        .flex()
        .flex_col()
        .min_h_0()
        .overflow_y_scroll();
    let mut at = 0;
    for row in rows {
        panel = panel.child(match row {
            FlyRow::Row(button) => {
                let mut row = button.menu().render(shell, cx);
                if let Some(focus) = popup.sub.rows.get(at) {
                    row = row.track_focus(focus);
                }
                at += 1;
                row.into_any_element()
            }
            FlyRow::Line => div()
                .flex_none()
                .my_1()
                .h(px(1.))
                .bg(theme.border)
                .into_any_element(),
        });
    }
    div()
        .id("popup-submenu")
        .role(gpui::Role::Menu)
        .aria_label(name.to_owned())
        .key_context("ToolbarSubmenu")
        .relative()
        .min_w(sized(180.))
        .max_w(sized(460.))
        .p_1()
        .flex()
        .flex_col()
        .min_h_0()
        .rounded_lg()
        .border_1()
        .border_color(theme.border)
        .bg(theme.surface)
        .text_color(theme.text)
        .shadow_md()
        .occlude()
        // The pointer reaching it keeps it open.
        .on_mouse_move(cx.listener(|this, _, _, _| {
            if let Some(popup) = this.submenu_popup() {
                popup.sub.rest = None;
            }
        }))
        .child(panel)
        .child(popup.sub.area.marker())
        .into_any_element()
}

impl Popup {
    pub fn new(kind: &'static str, window: &mut Window, cx: &mut Context<Shell>) -> Self {
        let previous = crate::modal::FocusReturn::capture(window, cx);
        // gpui puts a group's own focus inside the group (`popup_panel`),
        // ordered among its rows by its index: first, so the arrows and Tab
        // go on to the first row, never past them out of the popup.
        let focus = cx.focus_handle().tab_stop(false);
        window.focus(&focus, cx);
        Self {
            kind,
            focus,
            previous,
            scroll: ScrollHandle::new(),
            sub: Submenu::default(),
        }
    }
    pub fn restore(self, window: &mut Window, cx: &mut gpui::App) {
        self.previous.restore_if_any(window, cx);
    }
}

pub fn popup_panel(
    popup: &Popup,
    close: &'static str,
    theme: &Theme,
    cx: &mut Context<Shell>,
) -> Stateful<Div> {
    // What its submenu covers is part of it (`Submenu::area`).
    let beside = popup.sub.area.clone();
    // A row the keyboard reached is brought into view in this popup.
    if let Some(reveal) = cx.try_global::<RevealRow>()
        && reveal.row.is_some()
    {
        cx.global_mut::<RevealRow>().scroll = Some(popup.scroll.clone());
    }
    div()
        .id("toolbar-popup")
        .role(gpui::Role::Menu)
        .tab_group()
        .tab_index(1)
        .w(sized(264.))
        .min_h_0()
        .overflow_y_scroll()
        .track_scroll(&popup.scroll)
        .p(sized(POPUP_PAD))
        .flex()
        .flex_col()
        .gap_1()
        .rounded_lg()
        .border_1()
        .border_color(theme.border)
        .bg(theme.bg)
        .text_color(theme.text)
        .shadow_md()
        .occlude()
        .track_focus(&popup.focus)
        .tab_stop(false)
        .key_context("ToolbarPopup")
        .on_action(cx.listener(move |this, action: &RunCommand, window, cx| {
            if action.name == "ui.toolbar_close" {
                this.run(close, serde_json::json!({"close":true}), window, cx);
            } else {
                cx.propagate();
            }
        }))
        // A press outside closes it, and says so: the click that press
        // begins, on the button that opened it, must not open it again
        // (`Shell::note_dismissed`).
        .on_mouse_down_out(
            cx.listener(move |this, event: &gpui::MouseDownEvent, window, cx| {
                if beside
                    .bounds()
                    .is_some_and(|area| area.contains(&event.position))
                {
                    return;
                }
                this.run(
                    close,
                    serde_json::json!({"close": true, "pointer": true}),
                    window,
                    cx,
                );
            }),
        )
}

/// A popup that is a menu, drawn as a context menu is (`menu::render`):
/// on the chrome, its rows (`Button::menu`) with no room between them.
pub fn menu_panel(
    popup: &Popup,
    close: &'static str,
    theme: &Theme,
    cx: &mut Context<Shell>,
) -> Stateful<Div> {
    // As wide as its widest row, keys included, within bounds: a long
    // path keeps both its ends (`Button::label`).
    popup_panel(popup, close, theme, cx)
        .w_auto()
        .min_w(sized(240.))
        .max_w(sized(460.))
        .p_1()
        .gap_0()
        .bg(theme.surface)
}

/// What a popup that is a menu holds, top to bottom (`menu_parts`).
pub enum MenuPart {
    /// A group's name, with a button at its end (View's Buttons…).
    Title(SharedString, Option<Button>),
    /// A group's name that turns the group on or off (`Button::switch`):
    /// the whole heading is pressed, its switch at its end, and the
    /// group's rows wait, dimmed, while it is off. On and off is
    /// otherwise a check on a row.
    Heading(Button),
    /// A line between groups.
    Line,
    /// A row (`Button::menu`).
    Row(Button),
    /// A setting named, its values after its name (`Button::segment`).
    Choice(SharedString, Vec<Button>),
    /// Values as tiles, four to a row (`Button::tile`).
    Tiles(Vec<Button>),
    /// The row of a submenu of the popup open, by its name, called by its
    /// label (`submenu_row`): its rows beside it (`Shell::submenu_rows`).
    Sub(SharedString, SharedString),
}

/// `panel` holding `parts`, as a context menu holds its rows.
pub fn menu_parts(
    mut panel: Stateful<Div>,
    parts: Vec<MenuPart>,
    shell: &Shell,
    theme: &Theme,
    cx: &mut Context<Shell>,
) -> Stateful<Div> {
    for part in parts {
        panel = panel.child(match part {
            MenuPart::Title(title, button) => div()
                .flex()
                .flex_none()
                .items_center()
                .justify_between()
                .h(sized(26.))
                .pl_2()
                .pt_1()
                .text_small()
                .text_color(theme.text_muted)
                .child(title)
                .children(button.map(|button| button.render(shell, cx).h(sized(22.))))
                .into_any_element(),
            MenuPart::Heading(button) => button.render(shell, cx).mt_1().into_any_element(),
            MenuPart::Line => div()
                .flex_none()
                .my_1()
                .h(px(1.))
                .bg(theme.border)
                .into_any_element(),
            MenuPart::Row(button) => button.render(shell, cx).into_any_element(),
            MenuPart::Sub(name, label) => match shell.toolbar_menu.as_ref() {
                Some(popup) => submenu_row(
                    popup,
                    format!("popup-submenu-{name}"),
                    &name,
                    "",
                    label,
                    shell,
                    cx,
                ),
                None => div().into_any_element(),
            },
            MenuPart::Choice(name, values) => div()
                .flex()
                .flex_none()
                .items_center()
                .gap_2()
                .h(sized(MENU_ROW + 4.))
                .px_2()
                .child(div().size(px(16.)).flex_none())
                .child(
                    div()
                        .flex_1()
                        .pr_4()
                        .text_body()
                        .text_color(theme.text)
                        .child(name.clone()),
                )
                // A setting with no values here says only its name (Group by
                // while not grouped by a criterion of its own).
                .when(!values.is_empty(), |row| {
                    row.child(
                        // To a screen reader, the setting is a radio group named
                        // by its row, its values radio buttons (ADR-099), as a
                        // context menu's setting (`menu::choice_element`).
                        div()
                            .id(crate::elements::known_by("popup-choice", &name))
                            .role(gpui::Role::RadioGroup)
                            .when(shell.a11y, |d| d.aria_label(name))
                            .flex()
                            .flex_none()
                            .gap(sized(2.))
                            .p(sized(2.))
                            .rounded_md()
                            .border_1()
                            .border_color(theme.border)
                            .bg(theme.bg)
                            .children(values.into_iter().map(|value| value.render(shell, cx))),
                    )
                })
                .into_any_element(),
            MenuPart::Tiles(values) => div()
                .id("popup-tiles")
                .role(gpui::Role::RadioGroup)
                .grid()
                .grid_cols(4)
                .gap(sized(2.))
                .pb_1()
                .children(values.into_iter().map(|value| value.render(shell, cx)))
                .into_any_element(),
        });
    }
    panel
}

/// The flag a value's key context carries (`Button::segment`,
/// `Button::tile`): Left and Right walk to the values beside it.
pub(crate) const CHOICE: &str = "choice";
/// The key context's flag on a popup's row that opens a submenu
/// (`Button::opens_beside`).
pub(crate) const SUBMENU: &str = "submenu";

/// The key a button's id goes under in its key context.
const ROW: &str = "row";

/// The room around a popup's rows, which a row brought into view keeps.
pub(crate) const POPUP_PAD: f32 = 8.;

/// The row of the open popup the keyboard just reached
/// (`Shell::reveal_focused_row`), and the scroll of the popup drawing it
/// (`popup_panel`). Popups open one at a time: one is enough.
#[derive(Default)]
pub(crate) struct RevealRow {
    row: Option<SharedString>,
    scroll: Option<ScrollHandle>,
}

impl gpui::Global for RevealRow {}

/// Drawn in the row `RevealRow` names: in prepaint, where the row's
/// bounds are real, it scrolls the popup the least that shows the row
/// whole, once, and asks for the frame that draws it there.
fn reveal_marker() -> gpui::AnyElement {
    gpui::canvas(
        |row, window, cx| {
            // Asked once, whatever comes of it.
            let scroll = cx
                .try_global::<RevealRow>()
                .and_then(|reveal| reveal.scroll.clone());
            cx.set_global(RevealRow::default());
            let Some(scroll) = scroll else {
                return;
            };
            let area = scroll.bounds();
            // Another element of the same id, not the popup's row.
            if row.left() < area.left() || row.right() > area.right() {
                return;
            }
            let pad = crate::sizes::Scale::of(window).px(POPUP_PAD);
            let (top, bottom) = (area.top() + pad, area.bottom() - pad);
            let mut offset = scroll.offset();
            if row.top() < top {
                offset.y += top - row.top();
            } else if row.bottom() > bottom {
                offset.y -= row.bottom() - bottom;
            } else {
                return;
            }
            scroll.set_offset(offset);
            // A refresh asked while drawing is dropped: the next frame is.
            window.on_next_frame(|window, _| window.refresh());
        },
        |_, (), _, _| {},
    )
    .absolute()
    .top_0()
    .left_0()
    .size_full()
    .into_any_element()
}

#[cfg(test)]
mod tests {
    /// An icon added to the assets but not to `asset`'s list draws nothing,
    /// silently: every bundled file must resolve.
    #[test]
    fn every_bundled_icon_resolves() {
        let dir = concat!(env!("CARGO_MANIFEST_DIR"), "/assets/toolbar");
        for entry in std::fs::read_dir(dir).unwrap() {
            let path = entry.unwrap().path();
            if path.extension().is_some_and(|e| e == "svg") {
                let name = path.file_name().unwrap().to_str().unwrap();
                assert!(
                    super::asset(&format!("toolbar/{name}")).is_some(),
                    "{name} is not in toolbar::asset"
                );
            }
        }
    }
}
