//! The window's title bar (ADR-138): the system's where it draws one, else
//! skwir's, where the setting says (`ui.title_bar`): above the window as
//! the system's would be, on a row of its own on the frame, or in the top
//! row, its buttons at the ends of what holds the window's own tools
//! (`Shell::corner_at`, `Shell::toggles_at`).
//!
//! What skwir asks of the system follows the setting: the system's bar
//! for Automatic and Above, none for the other two. What it draws follows
//! the answer (`window.window_decorations()`): where the system draws its
//! bar, skwir draws none of its own whatever the setting says (an X11
//! session without a compositor, a compositor that keeps its own), so a
//! window never has two; where it draws none (GNOME, Weston), skwir draws
//! the one the setting says, Automatic's being the top row, as GNOME's
//! own applications have it.
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
    WindowButton, WindowDecorations, div, prelude::*, px,
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

/// How tall the bar of skwir's own is above the window (`Drawn::Above`),
/// as a desktop's title bar.
const ABOVE_HEIGHT: f32 = 38.;

/// How tall the band of skwir's own is on the frame (`Drawn::Row`): its
/// buttons and a line of text.
const ROW_HEIGHT: f32 = 32.;

/// `ui.title_bar`: where the window's title and buttons are.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum TitleBar {
    /// The system's where it draws one, else the top row.
    #[default]
    Automatic,
    /// Above the window: the system's, else one of skwir's like it.
    Above,
    /// On a row of skwir's own, on the frame above its areas.
    Row,
    /// In the top row: the buttons at its ends, its empty parts moving
    /// the window.
    TopRow,
}

impl TitleBar {
    pub const VALUES: [(&'static str, &'static str); 4] = [
        ("auto", "Automatic"),
        ("above", "Above"),
        ("row", "On a row of its own"),
        ("top_row", "In the top row"),
    ];

    /// The setting's value read, Automatic for none or one unknown.
    pub fn read(value: Option<&str>) -> Self {
        match value {
            Some("above") => Self::Above,
            Some("row") => Self::Row,
            Some("top_row") => Self::TopRow,
            _ => Self::Automatic,
        }
    }

    /// What the system is asked for: its own bar where it is wanted (or
    /// may be: Automatic takes it where there is one), else none.
    pub fn asks(self) -> WindowDecorations {
        match self {
            Self::Automatic | Self::Above => WindowDecorations::Server,
            Self::Row | Self::TopRow => WindowDecorations::Client,
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
    Above,
    /// A band on the frame above the window's areas.
    Row,
    /// The buttons at the ends of the top row.
    TopRow,
}

/// What skwir draws, by what the setting says and what the system draws
/// (`Window::window_decorations`).
pub fn drawn(setting: TitleBar, decorations: Decorations, fullscreen: bool) -> Drawn {
    if fullscreen {
        return Drawn::None;
    }
    match (decorations, setting) {
        (Decorations::Server, _) => Drawn::None,
        (Decorations::Client { .. }, TitleBar::Automatic | TitleBar::TopRow) => Drawn::TopRow,
        (Decorations::Client { .. }, TitleBar::Above) => Drawn::Above,
        (Decorations::Client { .. }, TitleBar::Row) => Drawn::Row,
    }
}

/// What Automatic comes to, said beside the setting (`.now`): above where
/// the system drew its bar when asked, the top row where it drew none.
/// Asked for none, a system that draws one anyway says it would; one that
/// gave none says nothing of what it does when asked.
pub fn automatic_now(asked: TitleBar, decorations: Decorations) -> Option<&'static str> {
    match (asked.asks(), decorations) {
        (_, Decorations::Server) => Some("above"),
        (WindowDecorations::Server, Decorations::Client { .. }) => Some("top_row"),
        (WindowDecorations::Client, Decorations::Client { .. }) => None,
    }
}

/// The window's title bar as this frame draws it: read once, before
/// anything is built.
#[derive(Clone, Debug, Default)]
pub struct Frame {
    pub drawn: Drawn,
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
        let drawn = drawn(setting, window.window_decorations(), window.is_fullscreen());
        if drawn == Drawn::None {
            return Self::default();
        }
        let controls = window.window_controls();
        let offered = |button: &WindowButton| match button {
            WindowButton::Close => true,
            WindowButton::Maximize => controls.maximize,
            WindowButton::Minimize => controls.minimize,
        };
        let layout = cx.button_layout().unwrap_or_else(default_layout);
        let side = |buttons: &[Option<WindowButton>]| {
            buttons.iter().flatten().copied().filter(offered).collect()
        };
        Self {
            drawn,
            tiling: window.tiling(),
            start: side(&layout.left),
            end: side(&layout.right),
            maximized: window.is_maximized(),
            active: window.is_window_active(),
        }
    }

    /// Whether skwir draws the window's margin, shadow and corners: while
    /// it draws its title bar, the system drawing no frame.
    pub fn framed(&self) -> bool {
        self.drawn != Drawn::None
    }

    /// The margin on each of the window's edges: none where it meets
    /// another window or the screen's.
    pub fn margin(&self) -> gpui::Edges<f32> {
        margin(self.framed(), self.tiling)
    }
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
/// the window is not fullscreen (`drawn`).
pub fn body(window: &Window) -> gpui::Bounds<gpui::Pixels> {
    let framed = matches!(window.window_decorations(), Decorations::Client { .. })
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

/// Where no desktop says (macOS and Windows draw their own; a desktop that
/// names no layout): Close alone at the end, as GNOME has it.
fn default_layout() -> gpui::WindowButtonLayout {
    let mut right = [None; gpui::MAX_BUTTONS_PER_SIDE];
    right[0] = Some(WindowButton::Close);
    gpui::WindowButtonLayout {
        left: [None; gpui::MAX_BUTTONS_PER_SIDE],
        right,
    }
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
    let listed = match end {
        End::Start => &frame.start,
        End::End => &frame.end,
    };
    if listed.is_empty() {
        return None;
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

/// The bar of skwir's own: above the window (`Drawn::Above`), on its own
/// ground with a line under it, as a desktop's; or on the frame
/// (`Drawn::Row`). The window's title in its middle, its buttons at its
/// ends, the rest of it moving the window.
pub fn bar(shell: &Shell, cx: &mut Context<Shell>) -> Option<AnyElement> {
    let drawn = shell.title_bar.drawn;
    if !matches!(drawn, Drawn::Above | Drawn::Row) {
        return None;
    }
    let theme = cx.global::<Theme>().clone();
    let above = drawn == Drawn::Above;
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
            .children(start)
            // The title, in the middle of the bar whatever its ends hold:
            // cut where they leave it no room.
            .child(
                div()
                    .absolute()
                    .top_0()
                    .bottom_0()
                    .left(sized(96.))
                    .right(sized(96.))
                    .flex()
                    .items_center()
                    .justify_center()
                    .child(
                        div()
                            .min_w_0()
                            .truncate()
                            .text_color(color)
                            .when(above, |d| d.font_weight(gpui::FontWeight::SEMIBOLD))
                            .child(title),
                    ),
            )
            .child(div().flex_1())
            .children(end)
            .into_any_element(),
    )
}

/// The window drawn whole while skwir draws its frame (`Frame::framed`):
/// `body` within the margin, its corners rounded where they stand apart
/// and what it holds kept within them, its shadow under it while nothing
/// shows through it, and the margin's edges and corners sizing it.
pub fn framed(frame: &Frame, theme: &Theme, window: &mut Window, body: AnyElement) -> AnyElement {
    let inset = if frame.framed() { px(MARGIN) } else { px(0.) };
    window.set_client_inset(inset);
    if !frame.framed() {
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

    /// Where the system draws its bar, skwir draws none whatever the
    /// setting says: never two bars.
    #[test]
    fn the_system_bar_leaves_no_room_for_another() {
        for (value, _) in TitleBar::VALUES {
            let setting = TitleBar::read(Some(value));
            assert_eq!(drawn(setting, Decorations::Server, false), Drawn::None);
        }
    }

    /// Where it draws none, the setting says which of skwir's: Automatic
    /// the top row, as GNOME's applications have it.
    #[test]
    fn without_one_the_setting_chooses() {
        assert_eq!(drawn(TitleBar::Automatic, CLIENT, false), Drawn::TopRow);
        assert_eq!(drawn(TitleBar::Above, CLIENT, false), Drawn::Above);
        assert_eq!(drawn(TitleBar::Row, CLIENT, false), Drawn::Row);
        assert_eq!(drawn(TitleBar::TopRow, CLIENT, false), Drawn::TopRow);
        assert_eq!(
            drawn(TitleBar::Row, CLIENT, true),
            Drawn::None,
            "fullscreen"
        );
    }

    /// The system's bar is asked for where it is wanted or may be.
    #[test]
    fn what_is_asked() {
        assert_eq!(TitleBar::Automatic.asks(), WindowDecorations::Server);
        assert_eq!(TitleBar::Above.asks(), WindowDecorations::Server);
        assert_eq!(TitleBar::Row.asks(), WindowDecorations::Client);
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
            Some("above")
        );
        assert_eq!(automatic_now(TitleBar::Automatic, CLIENT), Some("top_row"));
        assert_eq!(
            automatic_now(TitleBar::Row, Decorations::Server),
            Some("above")
        );
        assert_eq!(automatic_now(TitleBar::Row, CLIENT), None);
    }

    /// The margin is on the edges that meet nothing.
    #[test]
    fn a_tiled_edge_has_no_margin() {
        let frame = Frame {
            drawn: Drawn::TopRow,
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
