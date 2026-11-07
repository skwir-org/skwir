//! Form controls shared by dialogs: switch, segmented choice, buttons, text
//! field, stepper parts, slider and key caps. They are presentational: the caller
//! owns the focus handle, attaches the click listener and decides the order
//! of keyboard traversal, so a control never knows which view it lives in.
//! Each says what it is to assistive technology (a switch and whether it is
//! on, a radio button and whether it is chosen); a control that draws no
//! words of its name (a switch, a choice, a dropdown) is named by its caller
//! (`aria_label`), from the label drawn beside it.

use crate::sizes::TextSizes;
use crate::sizes::sized;
use gpui::{
    AnyElement, Div, Entity, FocusHandle, SharedString, Stateful, div, prelude::*, px, svg,
};

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

/// An on/off switch. The whole pill is the hit target. It is drawn as an
/// icon is, 36 × 20 px at every text size (ADR-117): the row around it
/// grows with its text, the switch keeps its shape.
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
        .p(px(2.))
        .rounded_full()
        .border_1()
        .border_color(gpui::transparent_black())
        .focus(move |s| s.border_color(accent))
        .cursor_pointer()
        .child(pill(on, theme))
}

/// A switch as it is drawn, on or off: what `switch` holds, and what a
/// menu's heading that turns its group on or off ends with
/// (`toolbar::Button::switch`).
pub fn pill(on: bool, theme: &Theme) -> Div {
    div()
        .relative()
        .flex_none()
        .w(px(36.))
        .h(px(20.))
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
    dropdown_with(id, None, label, focus, theme)
}

/// A dropdown whose label has a picture before it (what the choice looks
/// like: a palette's colors).
pub fn dropdown_with(
    id: impl Into<SharedString>,
    picture: Option<AnyElement>,
    label: impl Into<SharedString>,
    focus: &FocusHandle,
    theme: &Theme,
) -> Stateful<Div> {
    let (accent, hover) = (theme.accent, theme.hover);
    let label = label.into();
    let shown = div()
        .flex()
        .items_center()
        .gap_2()
        .children(picture)
        .child(label.clone());
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
        .child(shown)
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

/// The knob's size: a picture's, the same at every text size (ADR-117).
const KNOB: f32 = 14.;
/// The track's thickness.
const TRACK: f32 = 4.;

/// What the pointer did to a slider.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Slide {
    /// Pressed on it: the knob goes where it was pressed.
    Press,
    /// Moved while held, anywhere in the window.
    Drag,
    /// Let go, wherever it was.
    Release,
}

/// What a slider says to its owner: what the pointer did, and where along
/// the track it is, from 0 (its start) to 1 (its end).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Slid {
    pub slide: Slide,
    pub at: f32,
}

/// What a slider tells, as its owner's listener takes it
/// (`cx.listener(|this, slid: &Slid, window, cx| …)`).
pub type OnSlide = std::rc::Rc<dyn Fn(&Slid, &mut gpui::Window, &mut gpui::App)>;

/// Where `x` is along a track laid out in `bounds`, from 0 to 1: the
/// knob's centre stays inside, half a knob from each end.
pub(crate) fn along(bounds: gpui::Bounds<gpui::Pixels>, x: gpui::Pixels) -> f32 {
    let start = f32::from(bounds.origin.x) + KNOB / 2.;
    let length = f32::from(bounds.size.width) - KNOB;
    if length <= 0. {
        return 0.;
    }
    ((f32::from(x) - start) / length).clamp(0., 1.)
}

/// A slider at `at` (0 to 1): a track filled up to its knob. A press on it
/// tells `Press`, then each move `Drag` and the release `Release`, where
/// the pointer is along it, whatever its owner makes of that (a value's
/// step, a time). `held`: its owner was told a press and no release, so
/// the moves are followed over the whole window, not over the slider
/// alone. It is a slider to assistive technology; its owner names it,
/// says its value, takes its keys and gives it a focus if it has one.
pub fn slider(
    id: impl Into<SharedString>,
    at: f32,
    held: bool,
    theme: &Theme,
    on_slide: OnSlide,
) -> Stateful<Div> {
    let at = at.clamp(0., 1.);
    let bounds = std::rc::Rc::new(std::cell::Cell::new(gpui::Bounds::<gpui::Pixels>::default()));
    let tell = |slide: Slide| {
        let (bounds, on_slide) = (bounds.clone(), on_slide.clone());
        move |position: gpui::Point<gpui::Pixels>, window: &mut gpui::Window, cx: &mut gpui::App| {
            on_slide(
                &Slid {
                    slide,
                    at: along(bounds.get(), position.x),
                },
                window,
                cx,
            );
        }
    };
    let (press, release, release_out, drag) = (
        tell(Slide::Press),
        tell(Slide::Release),
        tell(Slide::Release),
        tell(Slide::Drag),
    );
    let measured = bounds.clone();
    let accent = theme.accent;
    div()
        .id(id.into())
        .role(gpui::Role::Slider)
        .relative()
        .h(sized(HEIGHT))
        .min_w(px(KNOB * 3.))
        .rounded_md()
        .border_1()
        .border_color(gpui::transparent_black())
        .focus(move |s| s.border_color(accent))
        .cursor_pointer()
        .child(
            // Where the pointer is read, and, while held, the window's
            // moves followed (as a field's selection is, `input.rs`): a
            // drag that leaves the slider still moves its knob.
            gpui::canvas(
                move |b, _, _| measured.set(b),
                move |_, (), window, _| {
                    if held {
                        window.on_mouse_event(
                            move |event: &gpui::MouseMoveEvent, phase, window, cx| {
                                if phase == gpui::DispatchPhase::Bubble
                                    && event.pressed_button == Some(gpui::MouseButton::Left)
                                {
                                    drag(event.position, window, cx);
                                }
                            },
                        );
                    }
                },
            )
            .absolute()
            .size_full(),
        )
        .child(
            div()
                .absolute()
                .top_0()
                .bottom_0()
                .left(px(KNOB / 2.))
                .right(px(KNOB / 2.))
                .flex()
                .items_center()
                .child(
                    div()
                        .relative()
                        .w_full()
                        .h(px(TRACK))
                        .rounded_full()
                        .bg(theme.border)
                        .child(
                            div()
                                .absolute()
                                .top_0()
                                .left_0()
                                .h_full()
                                .w(gpui::relative(at))
                                .rounded_full()
                                .bg(theme.accent),
                        )
                        .child(
                            div()
                                .absolute()
                                .top(px((TRACK - KNOB) / 2.))
                                .left(gpui::relative(at))
                                .ml(px(-KNOB / 2.))
                                .size(px(KNOB))
                                .rounded_full()
                                .bg(theme.accent),
                        ),
                ),
        )
        .on_mouse_down(
            gpui::MouseButton::Left,
            move |event: &gpui::MouseDownEvent, window, cx| {
                press(event.position, window, cx);
                // The press is the slider's: nothing under it starts a band
                // or takes the focus away.
                cx.stop_propagation();
            },
        )
        .on_mouse_up(
            gpui::MouseButton::Left,
            move |event: &gpui::MouseUpEvent, window, cx| release(event.position, window, cx),
        )
        .on_mouse_up_out(
            gpui::MouseButton::Left,
            move |event: &gpui::MouseUpEvent, window, cx| {
                release_out(event.position, window, cx);
            },
        )
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

#[cfg(test)]
mod tests {
    use super::{KNOB, along};
    use gpui::{Bounds, point, px, size};

    #[test]
    fn a_slider_is_read_between_its_knob_s_two_ends() {
        let bounds = Bounds::new(point(px(100.), px(0.)), size(px(200.), px(28.)));
        let start = 100. + KNOB / 2.;
        let length = 200. - KNOB;
        assert_eq!(along(bounds, px(start)), 0.);
        assert_eq!(along(bounds, px(start + length)), 1.);
        assert!((along(bounds, px(start + length / 4.)) - 0.25).abs() < 1e-6);
        // Past an end, the end.
        assert_eq!(along(bounds, px(0.)), 0.);
        assert_eq!(along(bounds, px(900.)), 1.);
        // A slider with no room for its knob is at its start.
        let narrow = Bounds::new(point(px(0.), px(0.)), size(px(KNOB), px(28.)));
        assert_eq!(along(narrow, px(5.)), 0.);
    }
}
