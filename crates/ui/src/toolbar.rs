//! Shared command-backed toolbar buttons, bundled symbolic icons and popovers.
use crate::{keymap::RunCommand, shell::Shell, theme::Theme};
use bibli_core::workspace::PaneId;
use gpui::{
    AnyView, App, Context, Div, FocusHandle, Render, SharedString, Stateful, Window, div,
    prelude::*, px, svg,
};

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
            shape: Shape::Tool,
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
    pub fn active(mut self, active: bool) -> Self {
        self.active = active;
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
            .key_context("ToolbarButton")
            .when(self.active, |d| {
                d.bg(theme.selection_bg).text_color(theme.selection_text)
            })
            .when(enabled, |d| {
                d.cursor_pointer()
                    .hover(|s| s.bg(theme.hover).text_color(theme.text))
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
                        .text_color(if self.active {
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
            .on_click(cx.listener(move |this, _, window, cx| {
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
    if !matches!(command, "ui.history_menu" | "ui.subfolder_menu") {
        shell.close_nav_menu(window);
    }
    if !matches!(
        command,
        "ui.toolbar_menu" | "ui.reader_menu" | "ui.preview_menu"
    ) {
        if let Some(popup) = shell.toolbar_menu.take() {
            popup.restore(window);
        }
        if let Some(popup) = shell.preview.menu.take() {
            popup.restore(window);
        }
    }
    if command != "ui.search_chip" {
        shell.close_search_chip(window);
    }
    shell.note_pointer_use(command, &args, cx);
    if !command.starts_with("ui.") {
        args["pane"] = serde_json::json!(pane);
    }
    shell.run(command, args, window, cx);
}

impl Shell {
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
pub fn row(theme: &Theme) -> Div {
    div()
        .tab_group()
        .relative()
        .w_full()
        .h(px(42.))
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
    div()
        .id("toolbar-popup")
        .tab_group()
        .tab_index(1)
        .w(px(264.))
        .min_h_0()
        .overflow_y_scroll()
        .p_2()
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
