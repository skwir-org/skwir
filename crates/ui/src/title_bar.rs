//! The window's title bar (ADR-138): the system's where it draws one, else
//! skwir's, where the setting says (`ui.title_bar`): above the window as
//! the system's would be (System), a compact one on the frame, or in the top
//! row, its buttons at the ends of what holds the window's own tools
//! (`Shell::corner_at`, `Shell::toggles_at`).
//!
//! What skwir asks of the system follows the setting: the system's bar
//! for Automatic and System, none for the other two. What it draws follows
//! the answer (`window.window_decorations()`), never shown before it comes
//! (`Window::decorations_pending`): where the system frames the window,
//! Automatic and System draw nothing of skwir's. A Wayland compositor that
//! keeps its frame though asked for none (Hyprland's: borders, no title
//! bar; Wayland cannot say whether a frame has one) gets skwir's bar
//! inside it for the two values that ask for skwir's; an X11 window
//! manager that keeps its own (no compositor runs) has a title bar, and
//! skwir draws none. Where the system draws no frame (GNOME, Weston),
//! skwir draws its frame and the bar the setting says, Automatic's being
//! the top row, as GNOME's own applications have it. Beside the setting,
//! what a refusal means here (`refusal`).
//!
//! Drawn by skwir, the window keeps what the system's frame gave it: its
//! buttons, from the desktop's layout (`App::button_layout`, GNOME's
//! `button-layout`) and only those the compositor offers
//! (`Window::window_controls`); its empty parts moving it
//! (`elements::window_drag`); a margin around it for its shadow, where its
//! edges are pressed to size it, with its corners rounded, on the edges
//! that meet no other window or the screen's (`Window::tiling`); nothing of
//! it in fullscreen.

use gpui::{
    AnyElement, App, Context, CursorStyle, Decorations, MouseButton, ResizeEdge, Tiling, Window,
    WindowButton, WindowControlArea, WindowDecorations, div, prelude::*, px,
};

use crate::shell::Shell;
use crate::sizes::sized;
use crate::theme::Theme;
use crate::toolbar::Button;

/// The window's margin while skwir draws its frame: the room of its
/// shadow, the edges pressed to size it. Pointer geometry: it keeps its
/// size whatever the text's.
pub const MARGIN: f32 = 12.;

/// How far the window's own corners are rounded where they stand apart:
/// as a card's (`frame::RADIUS_REMS`), whose corners on the window's edge
/// they round.
const RADIUS_REMS: f32 = crate::elements::frame::RADIUS_REMS;

/// How tall the bar of skwir's own is above the window (`Drawn::System`),
/// as a desktop's title bar.
const ABOVE_HEIGHT: f32 = 38.;

/// Whether skwir draws the window's frame (margin, shadow, corners,
/// edges) while it draws its title bar: on the free desktops, whose
/// compositor then draws none. Windows keeps its frame with the title bar
/// hidden, macOS with the title bar transparent.
const DRAWS_FRAME: bool = cfg!(any(target_os = "linux", target_os = "freebsd"));

/// Where macOS's traffic lights are, from the window's left edge, and the
/// room left for them at the start of the row that holds them: three
/// lights of 14 px, 6 px apart, and a gap after.
const LIGHTS_X: f32 = 14.;
const LIGHTS_ROOM: f32 = LIGHTS_X + 3. * 14. + 2. * 6. + 8.;

/// How wide a Windows caption button is, as Windows draws its own.
const CAPTION_WIDTH: f32 = 46.;

/// How tall the band of skwir's own is on the frame (`Drawn::Compact`): its
/// buttons and a line of text.
const ROW_HEIGHT: f32 = 32.;

/// `ui.title_bar`: where the window's title and buttons are.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum TitleBar {
    /// The system's where it draws one, else the top row.
    #[default]
    Automatic,
    /// The system's, else one of skwir's like it, above the window.
    System,
    /// Skwir's own, slim, on the frame above its areas.
    Compact,
    /// In the top row: the buttons at its ends, its empty parts moving
    /// the window.
    TopRow,
}

impl TitleBar {
    pub const VALUES: [(&'static str, &'static str); 4] = [
        ("auto", "Automatic"),
        ("system", "System"),
        ("compact", "Compact"),
        ("top_row", "In the top row"),
    ];

    /// The setting's value read, Automatic for none or one unknown.
    pub fn read(value: Option<&str>) -> Self {
        match value {
            Some("system") => Self::System,
            Some("compact") => Self::Compact,
            Some("top_row") => Self::TopRow,
            _ => Self::Automatic,
        }
    }

    /// What the system is asked for: its own bar where it is wanted (or
    /// may be: Automatic takes it where there is one), else none.
    pub fn asks(self) -> WindowDecorations {
        match self {
            Self::Automatic | Self::System => WindowDecorations::Server,
            Self::Compact | Self::TopRow => WindowDecorations::Client,
        }
    }
}

/// What skwir draws of the window's title bar this frame.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Drawn {
    /// Nothing: the system draws it, or the window is fullscreen.
    #[default]
    None,
    /// A bar above the window, as the system's would be.
    System,
    /// A slim band on the frame above the window's areas.
    Compact,
    /// The buttons at the ends of the top row.
    TopRow,
}

/// What skwir draws, by what the setting says and what the system answered
/// (`Window::window_decorations`). `x11`: an X11 window manager keeping
/// its frame keeps its title bar.
pub fn drawn(setting: TitleBar, decorations: Decorations, fullscreen: bool, x11: bool) -> Drawn {
    if fullscreen {
        return Drawn::None;
    }
    match (decorations, setting) {
        (Decorations::Client { .. }, TitleBar::Automatic | TitleBar::TopRow) => Drawn::TopRow,
        (Decorations::Client { .. }, TitleBar::System) => Drawn::System,
        (Decorations::Client { .. }, TitleBar::Compact) => Drawn::Compact,
        // Asked for none, the compositor kept its frame: skwir's bar
        // inside it, but where it is a window manager's with its title bar.
        (Decorations::Server, TitleBar::TopRow) if !x11 => Drawn::TopRow,
        (Decorations::Server, TitleBar::Compact) if !x11 => Drawn::Compact,
        (Decorations::Server, _) => Drawn::None,
    }
}

/// What a refusal means here, said beside the setting: asked for no frame,
/// the system kept its own.
pub fn refusal(asked: TitleBar, decorations: Decorations, x11: bool) -> Option<&'static str> {
    // On X11 the window manager keeps its frame where no compositor runs,
    // said before anything is asked (`UNSUPPORTED`).
    if x11
        || asked.asks() != WindowDecorations::Client
        || !matches!(decorations, Decorations::Server)
    {
        return None;
    }
    Some(
        "Here the desktop keeps its own frame around windows, whatever skwir asks: Compact and In the top row draw skwir's bar inside it, its corners, shadow and edges the desktop's. Should the desktop draw a title bar too, choose Automatic.",
    )
}

/// Why Compact and In the top row do nothing where the application's
/// decorations cannot be had (`Window::client_decorations_supported`: X11
/// with no compositor), said as they are offered.
pub const UNSUPPORTED: &str = "No compositor runs: the window manager draws the title bar";

/// What Automatic comes to, said beside the setting (`.now`): above where
/// the system drew its bar when asked, the top row where it drew none.
/// Asked for none, a system that draws one anyway says it would; one that
/// gave none says nothing of what it does when asked.
pub fn automatic_now(asked: TitleBar, decorations: Decorations) -> Option<&'static str> {
    match (asked.asks(), decorations) {
        (_, Decorations::Server) => Some("system"),
        (WindowDecorations::Server, Decorations::Client { .. }) => Some("top_row"),
        (WindowDecorations::Client, Decorations::Client { .. }) => None,
    }
}

/// The window's title bar as this frame draws it: read once, before
/// anything is built.
#[derive(Clone, Debug, Default)]
pub struct Frame {
    /// The setting what is drawn answers: the one asked when the system
    /// last answered. While an answer waits (`Window::decorations_pending`),
    /// the decorations are those that answered it, and so is what is drawn
    /// of them; the rest (fullscreen, tiling, the buttons) is read anew.
    pub answered: TitleBar,
    pub drawn: Drawn,
    /// Whether skwir draws the window's frame: its margin, shadow and
    /// corners, the system drawing none.
    pub framed: bool,
    /// The edges that meet another window or the screen's.
    pub tiling: Tiling,
    /// The buttons at the start of the row, and at its end, in order.
    pub start: Vec<WindowButton>,
    pub end: Vec<WindowButton>,
    pub maximized: bool,
    pub active: bool,
}

impl Frame {
    pub fn of(setting: TitleBar, window: &Window, cx: &App) -> Self {
        let decorations = window.window_decorations();
        let fullscreen = window.is_fullscreen();
        let drawn = drawn(setting, decorations, fullscreen, is_x11(cx));
        if drawn == Drawn::None {
            return Self {
                answered: setting,
                ..Self::default()
            };
        }
        let framed =
            DRAWS_FRAME && matches!(decorations, Decorations::Client { .. }) && !fullscreen;
        let controls = window.window_controls();
        let offered = |button: &WindowButton| match button {
            WindowButton::Close => true,
            WindowButton::Maximize => controls.maximize,
            WindowButton::Minimize => controls.minimize,
        };
        let layout = platform_layout(cx);
        let side = |buttons: &[Option<WindowButton>]| {
            buttons.iter().flatten().copied().filter(offered).collect()
        };
        Self {
            answered: setting,
            drawn,
            framed,
            tiling: window.tiling(),
            start: side(&layout.left),
            end: side(&layout.right),
            maximized: window.is_maximized(),
            active: window.is_window_active(),
        }
    }

    /// The margin on each of the window's edges: none where it meets
    /// another window or the screen's.
    pub fn margin(&self) -> gpui::Edges<f32> {
        margin(self.framed, self.tiling)
    }
}

/// Whether the window is X11's, whose window manager keeping its frame
/// keeps its title bar.
pub fn is_x11(cx: &App) -> bool {
    cx.compositor_name() == "X11"
}

fn margin(framed: bool, tiling: Tiling) -> gpui::Edges<f32> {
    if !framed {
        return gpui::Edges::default();
    }
    let room = |tiled: bool| if tiled { 0. } else { MARGIN };
    gpui::Edges {
        top: room(tiling.top),
        right: room(tiling.right),
        bottom: room(tiling.bottom),
        left: room(tiling.left),
    }
}

/// Where the window's body is within what it draws: all of it, but for
/// its margin while skwir draws its frame. What is sized or placed by the
/// window's size (the arrangement, a panel's limits, a popup kept within
/// the window) reads this, never the viewport. Read from the window
/// alone: skwir draws its frame exactly where the system draws none and
/// the window is not fullscreen (`Frame::framed`).
pub fn body(window: &Window) -> gpui::Bounds<gpui::Pixels> {
    let framed = DRAWS_FRAME
        && matches!(window.window_decorations(), Decorations::Client { .. })
        && !window.is_fullscreen();
    let margin = margin(framed, window.tiling());
    let viewport = window.viewport_size();
    gpui::Bounds::new(
        gpui::point(px(margin.left), px(margin.top)),
        gpui::size(
            (viewport.width - px(margin.left + margin.right)).max(px(0.)),
            (viewport.height - px(margin.top + margin.bottom)).max(px(0.)),
        ),
    )
}

/// The buttons skwir draws, by system: the free desktop's layout (Close
/// alone at the end where it names none, as GNOME has it); Windows'
/// minimize, maximize and close at the end; none on macOS, whose traffic
/// lights are the system's, moved into skwir's row (`lights`).
fn platform_layout(cx: &App) -> gpui::WindowButtonLayout {
    let none = [None; gpui::MAX_BUTTONS_PER_SIDE];
    if cfg!(target_os = "macos") {
        return gpui::WindowButtonLayout {
            left: none,
            right: none,
        };
    }
    if cfg!(target_os = "windows") {
        return gpui::WindowButtonLayout {
            left: none,
            right: [
                Some(WindowButton::Minimize),
                Some(WindowButton::Maximize),
                Some(WindowButton::Close),
            ],
        };
    }
    cx.button_layout().unwrap_or_else(|| {
        let mut right = none;
        right[0] = Some(WindowButton::Close);
        gpui::WindowButtonLayout { left: none, right }
    })
}

/// Where macOS's traffic lights go while skwir draws its title bar: in
/// the middle of its first row, the frame's gutter above it where the
/// cards hang below one (`gutter`). `None` elsewhere.
#[cfg(target_os = "macos")]
pub fn lights(frame: &Frame, gutter: f32, window: &Window) -> Option<gpui::Point<gpui::Pixels>> {
    let (top, height) = match frame.drawn {
        Drawn::TopRow => (gutter, crate::address::PANE_ROW_HEIGHT),
        Drawn::Compact => (0., ROW_HEIGHT),
        Drawn::System | Drawn::None => return None,
    };
    let height = f32::from(sized(height).to_pixels(window.rem_size()));
    Some(gpui::point(px(LIGHTS_X), px(top + (height - 14.) / 2.)))
}

/// Which end of the top row.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum End {
    /// Before the sidebar's toggle and More.
    Start,
    /// After Split, Properties and Preview.
    End,
}

/// The window's buttons at `end` of the row that holds the window's tools
/// there, while they are in the top row (`Drawn::TopRow`).
pub fn top_row_buttons(shell: &Shell, end: End, cx: &mut Context<Shell>) -> Option<AnyElement> {
    if shell.title_bar.drawn != Drawn::TopRow {
        return None;
    }
    buttons(shell, end, cx)
}

/// Under a row that holds the window's tools while its buttons are in the
/// top row: its empty parts move the window (`elements::window_drag`).
/// `zooms`: a double click there maximizes it, unless it means something
/// of the row's own.
pub fn top_row_drag(shell: &Shell, zooms: bool) -> Option<impl IntoElement + use<>> {
    (shell.title_bar.drawn == Drawn::TopRow)
        .then(|| crate::elements::window_drag::area(&shell.window_press, zooms))
}

/// The window's buttons at `end`, in the desktop's order; none where it
/// puts none there.
fn buttons(shell: &Shell, end: End, cx: &mut Context<Shell>) -> Option<AnyElement> {
    let frame = &shell.title_bar;
    // macOS's traffic lights are the system's, moved into the row's start
    // (`lights`): their room is left there.
    if cfg!(target_os = "macos") {
        return (end == End::Start && frame.drawn != Drawn::None)
            .then(|| div().flex_none().w(px(LIGHTS_ROOM - 6.)).into_any_element());
    }
    let listed = match end {
        End::Start => &frame.start,
        End::End => &frame.end,
    };
    if listed.is_empty() {
        return None;
    }
    if cfg!(target_os = "windows") {
        let theme = cx.global::<Theme>().clone();
        // Flush with the window's edge, as Windows' own: the end of the row
        // holding them gives up its padding to them, so the window's corner
        // is Close's (maximized, its top meets the screen's: `tiling`).
        return Some(
            div()
                .flex_none()
                .h_full()
                .flex()
                .ml(sized(4.))
                .mr(-sized(6.))
                .children(
                    listed
                        .iter()
                        .map(|button| caption(shell, *button, frame.maximized, &theme)),
                )
                .into_any_element(),
        );
    }
    let buttons: Vec<AnyElement> = listed
        .iter()
        .map(|button| button_element(shell, *button, frame.maximized, cx))
        .collect();
    Some(
        div()
            .flex_none()
            .flex()
            .items_center()
            .gap(sized(6.))
            .when(end == End::Start, |d| d.mr(sized(6.)))
            .when(end == End::End, |d| d.ml(sized(6.)))
            .children(buttons)
            .into_any_element(),
    )
}

fn button_element(
    shell: &Shell,
    button: WindowButton,
    maximized: bool,
    cx: &mut Context<Shell>,
) -> AnyElement {
    let (id, icon, title, command) = match button {
        WindowButton::Close => ("window-close", "close", "Close", "ui.close_window"),
        WindowButton::Minimize => ("window-minimize", "minus", "Minimize", "ui.minimize_window"),
        WindowButton::Maximize if maximized => (
            "window-maximize",
            "window-restore",
            "Restore",
            "ui.maximize_window",
        ),
        WindowButton::Maximize => (
            "window-maximize",
            "window-maximize",
            "Maximize",
            "ui.maximize_window",
        ),
    };
    // Not a stop of the keyboard's: the window's own keys do what they do
    // (Ctrl+Shift+W, the desktop's), as no desktop's title bar is tabbed
    // through.
    Button::new(id, icon, title, command)
        .control()
        .no_stop()
        .render(shell, cx)
        .into_any_element()
}

/// A Windows caption button: as Windows draws its own, square, the width
/// of a caption's, Close red under the pointer. Windows presses it: told
/// what it is (`WindowControlArea`), it shows Snap Layouts over Maximize
/// and minimizes, maximizes or closes the window itself, as for its own
/// buttons; a click of skwir's would act twice.
fn caption(shell: &Shell, button: WindowButton, maximized: bool, theme: &Theme) -> AnyElement {
    let (id, icon, label, area) = match button {
        WindowButton::Close => ("caption-close", "close", "Close", WindowControlArea::Close),
        WindowButton::Minimize => (
            "caption-minimize",
            "minus",
            "Minimize",
            WindowControlArea::Min,
        ),
        WindowButton::Maximize if maximized => (
            "caption-maximize",
            "window-restore",
            "Restore",
            WindowControlArea::Max,
        ),
        WindowButton::Maximize => (
            "caption-maximize",
            "window-maximize",
            "Maximize",
            WindowControlArea::Max,
        ),
    };
    let close = button == WindowButton::Close;
    let (close_ground, on_close, hover) =
        (theme.caption_close, theme.on_caption_close, theme.hover);
    div()
        .id(id)
        .role(gpui::Role::Button)
        .when(shell.a11y, |d| d.aria_label(label))
        .window_control_area(area)
        .flex_none()
        .w(px(CAPTION_WIDTH))
        .h_full()
        .flex()
        .items_center()
        .justify_center()
        .text_color(theme.text)
        .hover(move |s| {
            if close {
                s.bg(close_ground).text_color(on_close)
            } else {
                s.bg(hover)
            }
        })
        .child(
            gpui::svg()
                .path(format!("toolbar/{icon}.svg"))
                .size(px(14.)),
        )
        .into_any_element()
}

/// The bar of skwir's own: above the window (`Drawn::System`), on its own
/// ground with a line under it, as a desktop's; or on the frame
/// (`Drawn::Compact`). The window's title in its middle, its buttons at its
/// ends, the rest of it moving the window.
pub fn bar(shell: &Shell, cx: &mut Context<Shell>) -> Option<AnyElement> {
    let drawn = shell.title_bar.drawn;
    if !matches!(drawn, Drawn::System | Drawn::Compact) {
        return None;
    }
    let theme = cx.global::<Theme>().clone();
    let above = drawn == Drawn::System;
    let start = buttons(shell, End::Start, cx);
    let end = buttons(shell, End::End, cx);
    let title = if shell.title.is_empty() {
        "skwir".to_owned()
    } else {
        format!("{} — skwir", shell.title)
    };
    let color = if shell.title_bar.active {
        theme.text
    } else {
        theme.text_muted
    };
    Some(
        div()
            .id("title-bar")
            .role(gpui::Role::TitleBar)
            .relative()
            .flex_none()
            .w_full()
            .h(sized(if above { ABOVE_HEIGHT } else { ROW_HEIGHT }))
            .px(sized(6.))
            .flex()
            .items_center()
            .whitespace_nowrap()
            .when(above, |d| {
                d.bg(theme.window_frame())
                    .border_b_1()
                    .border_color(theme.border)
            })
            .when(!above, |d| {
                d.when_some(theme.chrome_ground(), gpui::Styled::bg)
            })
            .child(crate::elements::window_drag::area(
                &shell.window_press,
                true,
            ))
            // The title in the middle of the bar, its ends as wide as each
            // other while there is room; where there is less, an end keeps
            // the room its buttons take (no end narrower than they are), the
            // title beside it, cut where none is left. As a desktop's header
            // bar: nothing measured, whatever the ends hold (Windows' three
            // captions, a desktop's layout with several on a side).
            .child(
                div()
                    .flex_1()
                    .h_full()
                    .flex()
                    .items_center()
                    .justify_start()
                    .children(start),
            )
            .child(
                div()
                    .min_w_0()
                    .truncate()
                    .text_color(color)
                    .when(above, |d| d.font_weight(gpui::FontWeight::SEMIBOLD))
                    .child(title),
            )
            .child(
                div()
                    .flex_1()
                    .h_full()
                    .flex()
                    .items_center()
                    .justify_end()
                    .children(end),
            )
            .into_any_element(),
    )
}

/// The window drawn whole while skwir draws its frame (`Frame::framed`):
/// `body` within the margin, its corners rounded where they stand apart
/// and what it holds kept within them, its shadow under it while nothing
/// shows through it, and the margin's edges and corners sizing it.
pub fn framed(frame: &Frame, theme: &Theme, window: &mut Window, body: AnyElement) -> AnyElement {
    let inset = if frame.framed { px(MARGIN) } else { px(0.) };
    window.set_client_inset(inset);
    if !frame.framed {
        return body;
    }
    let margin = frame.margin();
    let tiling = frame.tiling;
    let radius = gpui::rems(RADIUS_REMS);
    let body = div()
        .size_full()
        .overflow_hidden()
        .when(!(tiling.top || tiling.left), |d| d.rounded_tl(radius))
        .when(!(tiling.top || tiling.right), |d| d.rounded_tr(radius))
        .when(!(tiling.bottom || tiling.left), |d| d.rounded_bl(radius))
        .when(!(tiling.bottom || tiling.right), |d| d.rounded_br(radius))
        // The window's elevation (`Theme::window_shadow`), within the
        // margin. Under a ground that lets what is behind through, a shadow
        // would darken it: none then.
        .when(!tiling.is_tiled() && !theme.opacity.see_through(), |d| {
            d.shadow(theme.window_shadow(frame.active))
        })
        .child(body);
    div()
        .size_full()
        .relative()
        .pt(px(margin.top))
        .pr(px(margin.right))
        .pb(px(margin.bottom))
        .pl(px(margin.left))
        .children(resize_edges(margin))
        .child(body)
        .into_any_element()
}

/// The margin's edges and corners, each sizing the window from where it
/// is; none on an edge without margin.
fn resize_edges(margin: gpui::Edges<f32>) -> Vec<AnyElement> {
    // A corner reaches along its two edges further than the margin is
    // deep, as a frame's corner is grabbed.
    let corner = px(MARGIN * 2.);
    let edge = |id: &'static str, edge: ResizeEdge| {
        div().id(id).absolute().cursor(cursor(edge)).on_mouse_down(
            MouseButton::Left,
            move |_, window, cx| {
                cx.stop_propagation();
                window.start_window_resize(edge);
            },
        )
    };
    let (top, right, bottom, left) = (
        margin.top > 0.,
        margin.right > 0.,
        margin.bottom > 0.,
        margin.left > 0.,
    );
    let mut edges = Vec::new();
    if top {
        edges.push(
            edge("resize-top", ResizeEdge::Top)
                .top_0()
                .left_0()
                .right_0()
                .h(px(margin.top)),
        );
    }
    if bottom {
        edges.push(
            edge("resize-bottom", ResizeEdge::Bottom)
                .bottom_0()
                .left_0()
                .right_0()
                .h(px(margin.bottom)),
        );
    }
    if left {
        edges.push(
            edge("resize-left", ResizeEdge::Left)
                .top_0()
                .bottom_0()
                .left_0()
                .w(px(margin.left)),
        );
    }
    if right {
        edges.push(
            edge("resize-right", ResizeEdge::Right)
                .top_0()
                .bottom_0()
                .right_0()
                .w(px(margin.right)),
        );
    }
    if top && left {
        edges.push(
            edge("resize-top-left", ResizeEdge::TopLeft)
                .top_0()
                .left_0()
                .size(corner),
        );
    }
    if top && right {
        edges.push(
            edge("resize-top-right", ResizeEdge::TopRight)
                .top_0()
                .right_0()
                .size(corner),
        );
    }
    if bottom && left {
        edges.push(
            edge("resize-bottom-left", ResizeEdge::BottomLeft)
                .bottom_0()
                .left_0()
                .size(corner),
        );
    }
    if bottom && right {
        edges.push(
            edge("resize-bottom-right", ResizeEdge::BottomRight)
                .bottom_0()
                .right_0()
                .size(corner),
        );
    }
    edges
        .into_iter()
        .map(IntoElement::into_any_element)
        .collect()
}

fn cursor(edge: ResizeEdge) -> CursorStyle {
    match edge {
        ResizeEdge::Top | ResizeEdge::Bottom => CursorStyle::ResizeUpDown,
        ResizeEdge::Left | ResizeEdge::Right => CursorStyle::ResizeLeftRight,
        ResizeEdge::TopLeft | ResizeEdge::BottomRight => CursorStyle::ResizeUpLeftDownRight,
        ResizeEdge::TopRight | ResizeEdge::BottomLeft => CursorStyle::ResizeUpRightDownLeft,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const CLIENT: Decorations = Decorations::Client {
        tiling: Tiling {
            top: false,
            left: false,
            right: false,
            bottom: false,
        },
    };

    /// Where the system keeps its frame, Automatic and System draw nothing of
    /// skwir's; the two that ask for skwir's draw it inside a Wayland
    /// compositor's frame (Hyprland's has no title bar), never inside an X11
    /// window manager's, which has one.
    #[test]
    fn a_frame_kept_holds_skwirs_bar_only_where_asked_and_not_on_x11() {
        let server = Decorations::Server;
        for x11 in [false, true] {
            assert_eq!(drawn(TitleBar::Automatic, server, false, x11), Drawn::None);
            assert_eq!(drawn(TitleBar::System, server, false, x11), Drawn::None);
        }
        assert_eq!(drawn(TitleBar::TopRow, server, false, false), Drawn::TopRow);
        assert_eq!(
            drawn(TitleBar::Compact, server, false, false),
            Drawn::Compact
        );
        assert_eq!(drawn(TitleBar::TopRow, server, false, true), Drawn::None);
        assert_eq!(drawn(TitleBar::Compact, server, false, true), Drawn::None);
        assert_eq!(drawn(TitleBar::Compact, server, true, false), Drawn::None);
    }

    /// A refusal is said where the system kept its frame though asked for
    /// none, as what it means there.
    #[test]
    fn a_refusal_is_said_as_what_it_means_here() {
        assert_eq!(
            refusal(TitleBar::Automatic, Decorations::Server, false),
            None
        );
        assert_eq!(refusal(TitleBar::Compact, CLIENT, false), None, "granted");
        let wayland = refusal(TitleBar::TopRow, Decorations::Server, false).unwrap();
        assert!(wayland.contains("inside it"), "{wayland}");
        assert_eq!(
            refusal(TitleBar::Compact, Decorations::Server, true),
            None,
            "X11's said as the choices are offered"
        );
    }

    /// Where it draws none, the setting says which of skwir's: Automatic
    /// the top row, as GNOME's applications have it.
    #[test]
    fn without_one_the_setting_chooses() {
        assert_eq!(
            drawn(TitleBar::Automatic, CLIENT, false, false),
            Drawn::TopRow
        );
        assert_eq!(drawn(TitleBar::System, CLIENT, false, false), Drawn::System);
        assert_eq!(
            drawn(TitleBar::Compact, CLIENT, false, false),
            Drawn::Compact
        );
        assert_eq!(drawn(TitleBar::TopRow, CLIENT, false, false), Drawn::TopRow);
        assert_eq!(
            drawn(TitleBar::Compact, CLIENT, true, false),
            Drawn::None,
            "fullscreen"
        );
    }

    /// The system's bar is asked for where it is wanted or may be.
    #[test]
    fn what_is_asked() {
        assert_eq!(TitleBar::Automatic.asks(), WindowDecorations::Server);
        assert_eq!(TitleBar::System.asks(), WindowDecorations::Server);
        assert_eq!(TitleBar::Compact.asks(), WindowDecorations::Client);
        assert_eq!(TitleBar::TopRow.asks(), WindowDecorations::Client);
        assert_eq!(TitleBar::read(Some("sideways")), TitleBar::Automatic);
        for (value, _) in TitleBar::VALUES {
            let read = TitleBar::read(Some(value));
            assert!(value == "auto" || read != TitleBar::Automatic, "{value}");
        }
    }

    /// Automatic is said as what it comes to where that is known.
    #[test]
    fn automatic_is_said_where_known() {
        assert_eq!(
            automatic_now(TitleBar::Automatic, Decorations::Server),
            Some("system")
        );
        assert_eq!(automatic_now(TitleBar::Automatic, CLIENT), Some("top_row"));
        assert_eq!(
            automatic_now(TitleBar::Compact, Decorations::Server),
            Some("system")
        );
        assert_eq!(automatic_now(TitleBar::Compact, CLIENT), None);
    }

    /// The margin is on the edges that meet nothing.
    #[test]
    fn a_tiled_edge_has_no_margin() {
        let frame = Frame {
            drawn: Drawn::TopRow,
            framed: true,
            tiling: Tiling {
                top: false,
                left: true,
                right: false,
                bottom: true,
            },
            ..Frame::default()
        };
        let margin = frame.margin();
        assert_eq!(
            (margin.top, margin.left, margin.right, margin.bottom),
            (MARGIN, 0., MARGIN, 0.)
        );
        assert_eq!(Frame::default().margin(), gpui::Edges::default());
    }
}
