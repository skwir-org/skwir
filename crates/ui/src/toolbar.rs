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
        "back",
        "forward",
        "up",
        "list",
        "grid",
        "columns",
        "preview",
        "preview-bottom",
        "split",
        "split-rows",
        "usage",
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
        "plugin",
        "keyboard",
        "appearance",
        "sliders",
        "external",
        "chevron-right",
        "sort",
        "folder-plus",
        "command",
        "rename",
        "cut",
        "paste",
        "trash",
        "tag",
        "shelf",
        "eject",
        "info",
        "crop",
        "code",
        "skip-next",
        "skip-previous",
        "pin"
    )
}

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
    command: &'static str,
    args: serde_json::Value,
    label: Option<SharedString>,
    anchor: Option<crate::elements::popover::PopoverAnchor>,
    pub active: bool,
    pub enabled: bool,
    /// A row of a popup menu: full width, left-aligned, with an icon slot
    /// that stays when the icon is empty so labels line up.
    menu: bool,
}
impl Button {
    pub fn new(
        id: impl Into<SharedString>,
        icon: impl Into<SharedString>,
        title: impl Into<SharedString>,
        command: &'static str,
    ) -> Self {
        Self {
            id: id.into(),
            icon: icon.into(),
            title: title.into(),
            command,
            args: serde_json::json!({}),
            label: None,
            anchor: None,
            active: false,
            enabled: true,
            menu: false,
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
        self.menu = true;
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
        let command = self.command;
        let args = self.args.clone();
        let enabled = self.enabled;
        // The shortcut comes from the keymap, never from the title: it
        // follows what is bound, and goes when its plugin does.
        let title: SharedString = match shell.keys_for(command, &args) {
            Some(keys) => format!("{} · {keys}", self.title).into(),
            None => self.title,
        };
        div()
            .id(self.id)
            .relative()
            .h(px(30.))
            .min_w(px(30.))
            .flex_none()
            .px(px(6.))
            .flex()
            .items_center()
            .when(!self.menu, gpui::Styled::justify_center)
            .when(self.menu, |d| d.w_full().px_2())
            .gap(px(if self.menu { 10. } else { 6. }))
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
            .tooltip(move |_, cx| tooltip(title.clone(), font.clone(), cx))
            .when(self.menu && self.icon.is_empty(), |d| {
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
            .children(self.label)
            .children(self.anchor.map(|anchor| anchor.marker()))
            .on_click(cx.listener(move |this, _, window, cx| {
                if enabled {
                    invoke(this, pane, command, args.clone(), window, cx);
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
    shell.note_pointer_use(command, &args, cx);
    if !command.starts_with("ui.") {
        args["pane"] = serde_json::json!(pane);
    }
    shell.run(command, args, window, cx);
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
        .on_mouse_down_out(cx.listener(move |this, _, window, cx| {
            this.run(close, serde_json::json!({"close":true}), window, cx);
        }))
}
