//! Form controls shared by dialogs: switch, segmented choice, buttons, text
//! field, stepper parts and key caps. They are presentational: the caller
//! owns the focus handle, attaches the click listener and decides the order
//! of keyboard traversal, so a control never knows which view it lives in.
//! Each says what it is to assistive technology (a switch and whether it is
//! on, a radio button and whether it is chosen); a control that draws no
//! words of its name (a switch, a choice, a dropdown) is named by its caller
//! (`aria_label`), from the label drawn beside it.

use crate::sizes::TextSizes;
use crate::sizes::sized;
use gpui::{Div, Entity, FocusHandle, SharedString, Stateful, div, prelude::*, px, svg};

use crate::input::TextInput;
use crate::theme::Theme;

/// Height shared by every single-line control so rows align.
pub const HEIGHT: f32 = 28.;

fn icon(name: &str, color: gpui::Hsla, size: f32) -> gpui::Svg {
    svg()
        .path(format!("toolbar/{name}.svg"))
        .text_color(color)
        .size(px(size))
        .flex_none()
}

/// An on/off switch. The whole pill is the hit target.
pub fn switch(
    id: impl Into<SharedString>,
    on: bool,
    focus: &FocusHandle,
    theme: &Theme,
) -> Stateful<Div> {
    let accent = theme.accent;
    div()
        .id(id.into())
        .role(gpui::Role::Switch)
        .aria_toggled(toggled(on))
        .track_focus(focus)
        .flex_none()
        .p(sized(2.))
        .rounded_full()
        .border_1()
        .border_color(gpui::transparent_black())
        .focus(move |s| s.border_color(accent))
        .cursor_pointer()
        .child(
            div()
                .relative()
                .w(px(36.))
                .h(sized(20.))
                .rounded_full()
                .bg(if on { theme.accent } else { theme.border })
                .child(
                    div()
                        .absolute()
                        .top(px(3.))
                        .left(px(if on { 19. } else { 3. }))
                        .size(px(14.))
                        .rounded_full()
                        .bg(gpui::white()),
                ),
        )
}

/// The frame of a segmented choice; fill it with [`segment`]s.
pub fn segmented(id: impl Into<SharedString>, focus: &FocusHandle, theme: &Theme) -> Stateful<Div> {
    let accent = theme.accent;
    div()
        .id(id.into())
        .role(gpui::Role::RadioGroup)
        .track_focus(focus)
        .flex_none()
        .flex()
        .items_center()
        .gap(sized(2.))
        .p(sized(2.))
        .h(sized(HEIGHT))
        .rounded_md()
        .bg(theme.bg)
        .border_1()
        .border_color(theme.border)
        .focus(move |s| s.border_color(accent))
}

pub fn segment(
    id: impl Into<SharedString>,
    label: impl Into<SharedString>,
    selected: bool,
    theme: &Theme,
) -> Stateful<Div> {
    let hover = theme.hover;
    let label = label.into();
    div()
        .id(id.into())
        .role(gpui::Role::RadioButton)
        .aria_label(label.clone())
        .aria_toggled(toggled(selected))
        .h_full()
        .px_3()
        .flex()
        .items_center()
        .rounded_sm()
        // Room for `now`'s outline in every segment, so marking one moves
        // none.
        .border_1()
        .border_color(gpui::transparent_black())
        .text_body()
        .whitespace_nowrap()
        .cursor_pointer()
        .when(selected, |d| {
            d.bg(theme.selection_bg).text_color(theme.selection_text)
        })
        .when(!selected, |d| {
            d.text_color(theme.text_muted).hover(move |s| s.bg(hover))
        })
        .child(label)
}

/// A segment marked as what the automatic choice, chosen, comes to now:
/// outlined in dashes, as a toolbar's automatic value is (`toolbar`'s
/// `now`).
pub fn now(segment: Stateful<Div>, theme: &Theme) -> Stateful<Div> {
    segment.border_dashed().border_color(theme.accent)
}

fn toggled(on: bool) -> gpui::Toggled {
    if on {
        gpui::Toggled::True
    } else {
        gpui::Toggled::False
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum ButtonStyle {
    Plain,
    Danger,
}

pub fn button(
    id: impl Into<SharedString>,
    label: impl Into<SharedString>,
    leading: Option<&str>,
    style: ButtonStyle,
    focus: &FocusHandle,
    theme: &Theme,
) -> Stateful<Div> {
    let hover = theme.hover;
    let color = if style == ButtonStyle::Danger {
        theme.error
    } else {
        theme.text
    };
    let label = label.into();
    button_frame(id, &label, focus, theme)
        .text_color(color)
        .cursor_pointer()
        .hover(move |s| s.bg(hover))
        .children(leading.map(|name| icon(name, color, 14.)))
        .child(label)
}

/// A button said, not offered, while what it asked for is under way (a
/// plan's "Stopping…"): a button's frame, its label muted, no pointer,
/// nothing to press, disabled for assistive technology; it keeps `focus`
/// where the button had it.
pub fn inert_button(
    id: impl Into<SharedString>,
    label: impl Into<SharedString>,
    focus: &FocusHandle,
    theme: &Theme,
) -> Stateful<Div> {
    let label = label.into();
    button_frame(id, &label, focus, theme)
        .aria_disabled(true)
        .text_color(theme.text_muted)
        .child(label)
}

/// What a button and an inert one share: role, name, focus, frame.
fn button_frame(
    id: impl Into<SharedString>,
    label: &SharedString,
    focus: &FocusHandle,
    theme: &Theme,
) -> Stateful<Div> {
    let accent = theme.accent;
    div()
        .id(id.into())
        .role(gpui::Role::Button)
        .aria_label(label.clone())
        .track_focus(focus)
        .flex_none()
        .h(sized(HEIGHT))
        .px_3()
        .flex()
        .items_center()
        .gap(sized(6.))
        .rounded_md()
        .border_1()
        .border_color(theme.border)
        .bg(theme.bg)
        .text_body()
        .whitespace_nowrap()
        .focus(move |s| s.border_color(accent))
}

/// A borderless square button holding one symbolic icon, `label` its name
/// for assistive technology (the icon says nothing to it).
pub fn icon_button(
    id: impl Into<SharedString>,
    name: &str,
    label: impl Into<SharedString>,
    focus: &FocusHandle,
    theme: &Theme,
) -> Stateful<Div> {
    let (accent, hover) = (theme.accent, theme.hover);
    div()
        .id(id.into())
        .role(gpui::Role::Button)
        .aria_label(label.into())
        .track_focus(focus)
        .flex_none()
        .size(sized(HEIGHT))
        .flex()
        .items_center()
        .justify_center()
        .rounded_md()
        .border_1()
        .border_color(gpui::transparent_black())
        .cursor_pointer()
        .hover(move |s| s.bg(hover))
        .focus(move |s| s.border_color(accent))
        .child(icon(name, theme.text_muted, 15.))
}

/// The frame around a [`TextInput`]. The input keeps its own focus handle.
pub fn field(
    input: &Entity<TextInput>,
    width: Option<f32>,
    focused: bool,
    invalid: bool,
    leading: Option<&str>,
    theme: &Theme,
) -> Div {
    div()
        .h(sized(HEIGHT))
        .map(|d| match width {
            Some(width) => d.w(px(width)).flex_none(),
            None => d.flex_1().min_w_0(),
        })
        .px_2()
        .flex()
        .items_center()
        .gap(sized(6.))
        .rounded_md()
        .bg(theme.bg)
        .border_1()
        .border_color(if invalid {
            theme.error
        } else if focused {
            theme.accent
        } else {
            theme.border
        })
        .children(leading.map(|name| icon(name, theme.text_muted, 14.)))
        .child(
            div()
                .flex_1()
                .min_w_0()
                .overflow_hidden()
                .child(input.clone()),
        )
}

/// The closed state of a dropdown: current label and a chevron. Its
/// current label is its value to assistive technology.
pub fn dropdown(
    id: impl Into<SharedString>,
    label: impl Into<SharedString>,
    focus: &FocusHandle,
    theme: &Theme,
) -> Stateful<Div> {
    let (accent, hover) = (theme.accent, theme.hover);
    let label = label.into();
    div()
        .id(id.into())
        .role(gpui::Role::ComboBox)
        .aria_value(label.clone())
        .track_focus(focus)
        .relative()
        .flex_none()
        .h(sized(HEIGHT))
        .min_w(sized(150.))
        .pl_3()
        .pr_2()
        .flex()
        .items_center()
        .justify_between()
        .gap_2()
        .rounded_md()
        .border_1()
        .border_color(theme.border)
        .bg(theme.bg)
        .text_body()
        .cursor_pointer()
        .hover(move |s| s.bg(hover))
        .focus(move |s| s.border_color(accent))
        .child(label)
        .child(icon("chevron", theme.text_muted, 14.))
}

/// One key of a shortcut, drawn as a key cap.
pub fn keycap(label: impl Into<SharedString>, theme: &Theme) -> Div {
    div()
        .flex_none()
        .min_w(px(22.))
        .h(sized(20.))
        .px(sized(6.))
        .flex()
        .items_center()
        .justify_center()
        .rounded_sm()
        .border_1()
        .border_color(theme.border)
        .bg(theme.bg)
        .text_small()
        .text_color(theme.text)
        .child(label.into())
}

/// A small rounded status label ("Missing", "Restart required").
pub fn badge(label: impl Into<SharedString>, color: gpui::Hsla, theme: &Theme) -> Div {
    div()
        .flex_none()
        .px(sized(6.))
        .h(sized(18.))
        .flex()
        .items_center()
        .rounded_sm()
        .border_1()
        .border_color(theme.border)
        .text_small()
        .text_color(color)
        .whitespace_nowrap()
        .child(label.into())
}

pub fn glyph(name: &str, color: gpui::Hsla, size: f32) -> gpui::Svg {
    icon(name, color, size)
}
