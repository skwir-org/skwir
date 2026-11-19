//! Start and end in place of left and right (ADR-139, 12): the one place
//! that says which side a side is. The window is mirrored while it speaks
//! a language written right to left (`skwir_core::direction::window`; no
//! shipped language yet, the right-to-left pseudo-language in a debug
//! build): what starts a row stands at its right, what ends it at its left,
//! text is aligned at its start, icons that point a way point the other.
//!
//! - **Layout the element says** names start and end: `pad_start`,
//!   `margin_end`, `border_start`, `inset_start`, `rounded_start` (the
//!   [`Sides`] methods), never `pl`, `mr`, `border_l`, `left` or
//!   `rounded_l` for a side that belongs to the reading's direction. A side
//!   that is the screen's whatever the language (a window's edge, a
//!   document's own margin) stays physical, and says why.
//! - **What is placed by hand** in prepaint (a grid's tiles, a column's
//!   cells, a scroll bar, a popup's place) asks [`from_start`] and
//!   [`mirrored`]; a drag's distance along x, [`along`].
//! - **An icon** is drawn by [`symbol`], which mirrors the icons that point
//!   along the line or picture the window's sides ([`POINTING`]); media's,
//!   a clock's and logos are drawn as they are.
//! - **The root says the direction** (`window_direction`: gpui's
//!   `LayoutDirection`, inherited, so every row lays itself out from its
//!   right when mirrored), and only when mirrored: left to right is gpui's
//!   own where nothing says one, which it then answers without looking
//!   (`Window::layout_direction`). What runs left to right in every
//!   language (what a document lays out, media's controls and time, a
//!   shortcut) says so on its root (`left_to_right`, `shortcut`).

use gpui::{
    AbsoluteLength, Bounds, DefiniteLength, Length, Pixels, Point, Styled, Svg, Transformation,
    size, svg,
};

/// A step of gpui's spacing scale, as its named sizes count them: what
/// `pl_3` pads is `pad_start(step(3.))`, a quarter of the text's size.
pub fn step(n: f32) -> gpui::Rems {
    gpui::rems(n / 4.)
}

/// Whether the window is laid out right to left.
pub fn rtl() -> bool {
    skwir_core::direction::window().is_right_to_left()
}

/// The sides of an element by the reading's direction: the start is the
/// left in a window laid out left to right, the right in one mirrored.
pub trait Sides: Styled + Sized {
    /// Padding on the start side.
    fn pad_start(self, length: impl Clone + Into<DefiniteLength>) -> Self {
        if rtl() {
            self.pr(length)
        } else {
            self.pl(length)
        }
    }

    /// Padding on the end side.
    fn pad_end(self, length: impl Clone + Into<DefiniteLength>) -> Self {
        if rtl() {
            self.pl(length)
        } else {
            self.pr(length)
        }
    }

    /// Margin on the start side.
    fn margin_start(self, length: impl Clone + Into<Length>) -> Self {
        if rtl() {
            self.mr(length)
        } else {
            self.ml(length)
        }
    }

    /// Margin on the end side.
    fn margin_end(self, length: impl Clone + Into<Length>) -> Self {
        if rtl() {
            self.ml(length)
        } else {
            self.mr(length)
        }
    }

    /// What follows pushed to the end, as `ml_auto` pushes it right.
    fn margin_start_auto(self) -> Self {
        if rtl() {
            self.mr_auto()
        } else {
            self.ml_auto()
        }
    }

    /// An absolute element's distance from its parent's start.
    fn inset_start(self, length: impl Clone + Into<Length>) -> Self {
        if rtl() {
            self.right(length)
        } else {
            self.left(length)
        }
    }

    /// An absolute element's distance from its parent's end.
    fn inset_end(self, length: impl Clone + Into<Length>) -> Self {
        if rtl() {
            self.left(length)
        } else {
            self.right(length)
        }
    }

    /// The border on the start side.
    fn border_start(self, width: impl Clone + Into<AbsoluteLength>) -> Self {
        if rtl() {
            self.border_r(width)
        } else {
            self.border_l(width)
        }
    }

    /// The border on the end side.
    fn border_end(self, width: impl Clone + Into<AbsoluteLength>) -> Self {
        if rtl() {
            self.border_l(width)
        } else {
            self.border_r(width)
        }
    }

    /// Both corners of the start side rounded.
    fn rounded_start(self, radius: impl Clone + Into<AbsoluteLength>) -> Self {
        if rtl() {
            self.rounded_r(radius)
        } else {
            self.rounded_l(radius)
        }
    }

    /// Both corners of the end side rounded.
    fn rounded_end(self, radius: impl Clone + Into<AbsoluteLength>) -> Self {
        if rtl() {
            self.rounded_l(radius)
        } else {
            self.rounded_r(radius)
        }
    }

    /// The top corner of the start side rounded.
    fn rounded_top_start(self, radius: impl Clone + Into<AbsoluteLength>) -> Self {
        if rtl() {
            self.rounded_tr(radius)
        } else {
            self.rounded_tl(radius)
        }
    }

    /// The top corner of the end side rounded.
    fn rounded_top_end(self, radius: impl Clone + Into<AbsoluteLength>) -> Self {
        if rtl() {
            self.rounded_tl(radius)
        } else {
            self.rounded_tr(radius)
        }
    }

    /// The bottom corner of the start side rounded.
    fn rounded_bottom_start(self, radius: impl Clone + Into<AbsoluteLength>) -> Self {
        if rtl() {
            self.rounded_br(radius)
        } else {
            self.rounded_bl(radius)
        }
    }

    /// The bottom corner of the end side rounded.
    fn rounded_bottom_end(self, radius: impl Clone + Into<AbsoluteLength>) -> Self {
        if rtl() {
            self.rounded_bl(radius)
        } else {
            self.rounded_br(radius)
        }
    }

    /// Text aligned at its start, where the reading begins, as the element
    /// is laid out (gpui's `TextAlign::Start`, against the direction it
    /// inherits): what the window's root says for every text in it
    /// (`window_direction`).
    fn text_start(self) -> Self {
        self.text_align(gpui::TextAlign::Start)
    }

    /// Text aligned at its end (a number in a column, a size).
    fn text_end(self) -> Self {
        self.text_align(gpui::TextAlign::End)
    }

    /// The window's root: laid out in the window's direction (gpui's
    /// `LayoutDirection`, which every element inherits: a row's children
    /// from its right, mirrored), its texts aligned at their start. Left to
    /// right it says none: that is gpui's own where nothing says one, and
    /// while nothing does, every element's `Window::layout_direction` is
    /// answered without a walk of the text styles above it, which a
    /// direction said at the root would make each one take.
    fn window_direction(self) -> Self {
        let root = if rtl() {
            self.layout_direction(gpui::LayoutDirection::RightToLeft)
        } else {
            self
        };
        root.text_start()
    }

    /// The root of what is laid out left to right whatever the window's
    /// direction, never mirrored: what a document lays out (a text
    /// preview, code, a page, a terminal), which is the document's, and
    /// media's controls and time, which run so in every language. Its text
    /// is aligned at the left.
    fn left_to_right(self) -> Self {
        self.layout_direction(gpui::LayoutDirection::LeftToRight)
            .text_left()
    }
}

impl<E: Styled> Sides for E {}

/// The icons that point along the line (back, forward, a chevron that
/// opens to the side, undo and redo) or picture the window's sides (the
/// sidebar's panel, the preview's, a bulleted list): drawn mirrored in a
/// window laid out right to left. Media's (play, skip), a clock's, an
/// arrow that points up or down and what pictures a thing are drawn as
/// they are, as every platform's guidelines have it.
pub const POINTING: &[&str] = &[
    "back",
    "forward",
    "left",
    "right",
    "chevron-right",
    "redo",
    "revert",
    "sidebar",
    "preview",
    "contents",
    "list",
    "move",
];

/// One of the bundled symbolic icons (`toolbar::asset`), by its name:
/// mirrored in a window laid out right to left where it points a way
/// ([`POINTING`]).
pub fn symbol(name: &str) -> Svg {
    let drawn = svg().path(format!("toolbar/{name}.svg"));
    if rtl() && POINTING.contains(&name) {
        drawn.with_transformation(Transformation::scale(size(-1., 1.)))
    } else {
        drawn
    }
}

/// The sign between two steps of a path drawn as a row (an archive's
/// member in another, a search's pill, a folder that opens to the side):
/// `›` pointing toward the line's end, `‹` in a window mirrored. Drawn
/// alone, a sign has no letter to mirror it by.
pub fn onward() -> &'static str {
    if rtl() { "‹" } else { "›" }
}

/// The arrow from what was to what will be (a name before and after a
/// rename): `→`, `←` in a window mirrored.
pub fn becomes() -> &'static str {
    if rtl() { "←" } else { "→" }
}

/// Keys as a shortcut is drawn beside what it runs (a menu's row, the
/// palette's): left to right whatever the window's direction, as every
/// desktop writes Ctrl+Shift+C, isolated (LRI, PDI) so neither the row's
/// direction nor a key's name written right to left reorders its strokes.
/// As it is in a window laid out left to right. Only for what is drawn:
/// never words that leave the window.
pub fn shortcut(keys: String) -> String {
    if rtl() {
        format!("\u{2066}{keys}\u{2069}")
    } else {
        keys
    }
}

/// The left edge of something `width` wide placed `offset` from the start
/// of `within`: from its left in a window laid out left to right, from its
/// right in one mirrored. What prepaint places by hand (a tile, a cell)
/// asks this, never `within.left() + offset`.
pub fn from_start(within: Bounds<Pixels>, offset: Pixels, width: Pixels) -> Pixels {
    if rtl() {
        within.right() - offset - width
    } else {
        within.left() + offset
    }
}

/// `placed`, laid out from the left of `within`, put where it goes in the
/// window's direction: as it is left to right, reflected across `within`'s
/// middle right to left.
pub fn mirrored(within: Bounds<Pixels>, placed: Bounds<Pixels>) -> Bounds<Pixels> {
    if !rtl() {
        return placed;
    }
    let x = within.left() + within.right() - placed.right();
    Bounds {
        origin: Point {
            x,
            y: placed.origin.y,
        },
        size: placed.size,
    }
}

/// How far `x` is from the start edge of `within`: from its left in a
/// window laid out left to right, from its right in one mirrored. An edge
/// dragged at the end of an area that starts at `within`'s start (the
/// sidebar, the first of two panes) makes it this wide.
pub fn from_start_edge(within: Bounds<Pixels>, x: Pixels) -> Pixels {
    if rtl() {
        within.right() - x
    } else {
        x - within.left()
    }
}

/// How far `x` is from the end edge of `within`: an edge dragged at the
/// start of an area that ends at `within`'s end (the preview panel) makes
/// it this wide.
pub fn from_end_edge(within: Bounds<Pixels>, x: Pixels) -> Pixels {
    if rtl() {
        x - within.left()
    } else {
        within.right() - x
    }
}

/// The corner of a popup that hangs at its anchor, as the window reads: a
/// menu hung by its top left corner (under its button, reaching right) is
/// hung by its top right one in a window mirrored (reaching left).
pub fn corner(corner: gpui::Anchor) -> gpui::Anchor {
    use gpui::Anchor;
    if !rtl() {
        return corner;
    }
    match corner {
        Anchor::TopLeft => Anchor::TopRight,
        Anchor::TopRight => Anchor::TopLeft,
        Anchor::BottomLeft => Anchor::BottomRight,
        Anchor::BottomRight => Anchor::BottomLeft,
        Anchor::LeftCenter => Anchor::RightCenter,
        Anchor::RightCenter => Anchor::LeftCenter,
        centered => centered,
    }
}

/// A distance moved along x (a drag of an edge, the wheel sideways) as the
/// reading goes: rightward is forward left to right, backward mirrored.
pub fn along(dx: Pixels) -> Pixels {
    if rtl() { -dx } else { dx }
}

#[cfg(test)]
mod tests {
    use gpui::{Bounds, point, px, size};

    /// Left to right, what is placed by hand is where it was; mirrored, it
    /// is reflected across what holds it.
    #[test]
    fn what_is_placed_by_hand_is_reflected_only_when_mirrored() {
        let within = Bounds::new(point(px(100.), px(0.)), size(px(300.), px(50.)));
        let placed = Bounds::new(point(px(110.), px(5.)), size(px(40.), px(10.)));
        assert_eq!(super::mirrored(within, placed), placed);
        assert_eq!(super::from_start(within, px(10.), px(40.)), px(110.));
        assert_eq!(super::along(px(5.)), px(5.));
        assert_eq!(super::from_start_edge(within, px(130.)), px(30.));
        assert_eq!(super::from_end_edge(within, px(130.)), px(270.));
        skwir_core::locale::set_chosen(Some(skwir_core::direction::PSEUDO_RTL.into()));
        let reflected = super::mirrored(within, placed);
        assert_eq!(reflected.origin, point(px(350.), px(5.)));
        assert_eq!(reflected.size, placed.size);
        assert_eq!(super::from_start(within, px(10.), px(40.)), px(350.));
        assert_eq!(super::along(px(5.)), px(-5.));
        assert_eq!(super::from_start_edge(within, px(130.)), px(270.));
        assert_eq!(super::from_end_edge(within, px(130.)), px(30.));
        skwir_core::locale::set_chosen(None);
        assert!(!super::rtl());
    }

    /// The window's root says a direction only when mirrored: left to right
    /// is gpui's own where nothing says one, so it says none there, and
    /// gpui answers every element's direction without a walk.
    #[test]
    fn the_root_says_a_direction_only_when_mirrored() {
        use super::Sides as _;
        let said = || {
            let mut root = gpui::div().window_direction();
            gpui::Styled::style(&mut root).text.layout_direction
        };
        assert_eq!(said(), None);
        skwir_core::locale::set_chosen(Some(skwir_core::direction::PSEUDO_RTL.into()));
        assert_eq!(said(), Some(gpui::LayoutDirection::RightToLeft));
        skwir_core::locale::set_chosen(None);
        assert_eq!(said(), None);
    }
}
