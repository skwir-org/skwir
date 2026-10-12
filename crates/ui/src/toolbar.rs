//! Shared command-backed toolbar buttons, bundled symbolic icons and popovers.
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
        "map",
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
        "eye",
        "move",
        "refresh",
        "play",
        "pause",
        "volume",
        "muted",
        "fullscreen",
        "revert",
        "check",
        "warning",
        "link",
        "plugin",
        "keyboard",
        "appearance",
        "sliders",
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
        "terminal",
        "lock",
    )
}

/// The widest a tooltip grows: a long name (`elements::clip`) wraps.
const TIP_MAX: f32 = 480.;

struct Tip {
    text: SharedString,
    font: SharedString,
}
impl Render for Tip {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.global::<Theme>();
        div()
            .font_family(self.font.clone())
            .text_xs()
            .text_color(theme.text)
            .bg(theme.surface)
            .border_1()
            .border_color(theme.border)
            .rounded_md()
            .shadow_sm()
            .max_w(px(TIP_MAX))
            .px_2()
            .py_1()
            .child(self.text.clone())
    }
}
pub(crate) fn tooltip(text: SharedString, font: SharedString, cx: &mut App) -> AnyView {
    cx.new(|_| Tip { text, font }).into()
}

#[derive(Clone)]
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
        let font = shell.font.clone();
        let pane = shell.pane;
        let command = self.command.clone();
        let args = self.args.clone();
        let enabled = self.enabled;
        let menu = self.shape == Shape::MenuRow;
        // The shortcut comes from the keymap, never from the title: it
        // follows what is bound, and goes when its plugin does.
        let title: SharedString = match shell.keys_for(&command, &args) {
            Some(keys) => format!("{} · {keys}", self.title).into(),
            None => self.title,
        };
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
        div()
            .id(self.id)
            .relative()
            .h(px(30.))
            .min_w(px(30.))
            .flex_none()
            .px(px(6.))
            .flex()
            .items_center()
            .when(!menu, gpui::Styled::justify_center)
            .when(menu, |d| d.w_full().px_2())
            .gap(px(if menu { 10. } else { 6. }))
            .rounded_md()
            .border_1()
            .border_color(gpui::transparent_black())
            .text_xs()
            .whitespace_nowrap()
            .text_color(theme.text_muted)
            // Keep the handle alive when navigation disables this button.
            // Dropping the focused handle would also lose ancestor shortcuts.
            .focusable()
            .tab_index(0)
            .tab_stop(enabled)
            // Its id says which row of a popup the keyboard reached
            // (`Shell::reveal_focused_row`).
            .key_context({
                let mut context = gpui::KeyContext::default();
                context.add("ToolbarButton");
                context.set(ROW, id);
                context
            })
            .when(self.active, |d| {
                d.bg(theme.selection_bg).text_color(theme.selection_text)
            })
            .when_some(tint, |d, (fill, edge, _, _)| {
                d.bg(fill).border_color(edge).text_color(theme.text)
            })
            .when(enabled, |d| {
                d.cursor_pointer()
                    .hover(|s| match tint {
                        Some((_, _, fill, edge)) => s.bg(fill).border_color(edge),
                        None => s.bg(theme.hover).text_color(theme.text),
                    })
                    .focus(|s| s.border_color(theme.accent))
            })
            .when(!enabled, |d| d.opacity(0.35))
            .tooltip(move |_, cx| {
                let tip = clip
                    .as_ref()
                    .and_then(|clip| clip.says(Some(&title)))
                    .unwrap_or_else(|| title.clone());
                tooltip(tip, font.clone(), cx)
            })
            .when(menu && self.icon.is_empty(), |d| {
                d.child(div().size(px(16.)).flex_none())
            })
            .when(!self.icon.is_empty(), |d| {
                d.child(
                    svg()
                        .path(format!("toolbar/{}.svg", self.icon))
                        .text_color(if self.active && tint.is_none() {
                            theme.selection_text
                        } else {
                            theme.text_muted
                        })
                        .size(px(16.))
                        .flex_none(),
                )
            })
            // A label too long for a menu keeps both ends, as names do.
            .children(label.map(|label| {
                div()
                    .min_w_0()
                    .overflow_hidden()
                    .text_ellipsis_middle()
                    .child(label)
            }))
            .when(self.shape == Shape::Dropdown, |d| {
                d.gap(px(2.)).child(
                    svg()
                        .path("toolbar/chevron.svg")
                        .text_color(theme.text_muted)
                        .size(px(10.))
                        .flex_none(),
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
                                invoke(this, pane, &command, args, window, cx);
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
            })
            .on_click(cx.listener(move |this, _, window, cx| {
                // The release of a long press: what it held ran instead.
                if this
                    .held
                    .take()
                    .is_some_and(|press| press.id == clicked && press.fired)
                {
                    return;
                }
                if enabled {
                    invoke(this, pane, &command, args.clone(), window, cx);
                }
            }))
            .on_action(cx.listener(move |_, action: &RunCommand, _, cx| {
                // GPUI sends a keyboard click on key-up. Consume key-down so
                // a parent's Enter/Space binding cannot also activate an action.
                if action.name != "ui.toolbar_activate" {
                    cx.propagate();
                }
            }))
    }
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

fn invoke(
    shell: &mut Shell,
    pane: PaneId,
    command: &str,
    mut args: serde_json::Value,
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
    if let Some(was) = shell.close_popup(window) {
        shell.dismissed = Some((shell.presses, was));
    }
    shell.note_pointer_use(command, &args, cx);
    // A command acts on the button's pane, unless the button names the
    // pane it acts on (a location's tool for the other pane's folder).
    if !command.starts_with("ui.") && args.get("pane").is_none() {
        args["pane"] = serde_json::json!(pane);
    }
    shell.run(command, args, window, cx);
}

impl Shell {
    /// Close the popup open, whichever it is (More or the View menu, a
    /// menu of places, a search pill's choices, a preview's menu): there is
    /// one at most. Returns what it was, as its toggle names it. A menu of
    /// places still being read is no longer wanted (`nav_asked`).
    pub(crate) fn close_popup(&mut self, window: &mut Window) -> Option<String> {
        self.nav_asked += 1;
        let mut was = None;
        if let Some(popup) = self.toolbar_menu.take() {
            was = Some(popup.kind.to_owned());
            popup.restore(window);
        }
        if let Some(what) = self.close_nav_menu(window) {
            was = Some(what.key());
        }
        if let Some(key) = self.close_search_chip(window) {
            was = Some(key);
        }
        if let Some(popup) = self.preview.menu.take() {
            was = Some(format!("preview {}", popup.kind));
            popup.restore(window);
        }
        was
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
    ) -> bool {
        let was = self.close_popup(window);
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
            .find(|context| context.contains("ToolbarButton"))
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
    pub(crate) fn toolbar_popup_end(&mut self, last: bool, window: &mut Window, cx: &App) {
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
                window.focus_next();
            } else {
                window.focus_prev();
            }
            if !in_popup(window) || window.focused(cx).as_ref() == Some(&at) {
                window.focus(&at);
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
        .h(px(18.))
        .mx(px(5.))
        .flex_none()
        .bg(theme.border)
}
/// How tall a toolbar's row is.
pub const HEIGHT: f32 = 42.;

pub fn row(theme: &Theme) -> Div {
    div()
        .tab_group()
        .relative()
        .w_full()
        .h(px(HEIGHT))
        .flex()
        .items_center()
        .gap(px(2.))
        .px(px(6.))
        .bg(theme.surface)
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
}
impl Popup {
    pub fn new(kind: &'static str, window: &mut Window, cx: &mut Context<Shell>) -> Self {
        let previous = crate::modal::FocusReturn::capture(window, cx);
        let focus = cx.focus_handle().tab_index(1).tab_stop(false);
        window.focus(&focus);
        Self {
            kind,
            focus,
            previous,
            scroll: ScrollHandle::new(),
        }
    }
    pub fn restore(self, window: &mut Window) {
        self.previous.restore_if_any(window);
    }
}

pub fn popup_panel(
    popup: &Popup,
    close: &'static str,
    theme: &Theme,
    cx: &mut Context<Shell>,
) -> Stateful<Div> {
    // A row the keyboard reached is brought into view in this popup.
    if let Some(reveal) = cx.try_global::<RevealRow>()
        && reveal.row.is_some()
    {
        cx.global_mut::<RevealRow>().scroll = Some(popup.scroll.clone());
    }
    div()
        .id("toolbar-popup")
        .tab_group()
        .tab_index(1)
        .w(px(264.))
        .min_h_0()
        .overflow_y_scroll()
        .track_scroll(&popup.scroll)
        .p(px(POPUP_PAD))
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
        .on_mouse_down_out(cx.listener(move |this, _, window, cx| {
            this.run(
                close,
                serde_json::json!({"close": true, "pointer": true}),
                window,
                cx,
            );
        }))
}

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
            let (top, bottom) = (area.top() + px(POPUP_PAD), area.bottom() - px(POPUP_PAD));
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
