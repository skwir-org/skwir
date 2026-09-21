//! Theme tokens, chosen from the window appearance and stored as a GPUI global.
//!
//! Every surface draws from these and from the scales below, so a menu, a
//! toast, a dialog and a row agree without anyone comparing hex values:
//!
//! - surfaces: what is read (lists, text, code, tables, bytes, prose) is on
//!   `bg` in `text`; `surface` is chrome (bars, headers, the sidebar, the
//!   letterbox around a picture or a page). `code_bg` sets a code block or a
//!   table header apart on `bg`, `zebra` tints alternate rows. `text_muted`
//!   is for secondary facts (line numbers, captions), never for the data.
//! - color: the fields of [`Theme`]. State colors are tokens too: `hover` for
//!   whatever is under the pointer, `selection_*` for what is chosen,
//!   `success`/`warning`/`error` for outcomes, `scrim` behind a dialog.
//!   Do not derive a hover from `border` or `surface` locally.
//! - radius: `rounded_sm` inside controls (segments, menu rows, key caps),
//!   `rounded_md` for controls and rows, `rounded_lg` for panels (menus,
//!   toasts, cards, dialogs), `rounded_full` for pills.
//! - elevation: `shadow_md` for menus and popovers, `shadow_lg` for toasts
//!   and dialogs; nothing else casts a shadow.
//! - spacing: multiples of 4 px; rows are 26 px, controls 28 px, bars 42 px.

use gpui::{Global, Hsla, WindowAppearance, rgb};

/// The color tokens every module renders with.
#[derive(Clone, Debug)]
pub struct Theme {
    pub bg: Hsla,
    pub surface: Hsla,
    /// A code block, an inline code span or a table header, on `bg`.
    pub code_bg: Hsla,
    /// Alternate rows of a table or of a byte dump, on `bg`.
    pub zebra: Hsla,
    /// Background of the row or control under the pointer.
    pub hover: Hsla,
    pub border: Hsla,
    pub text: Hsla,
    pub text_muted: Hsla,
    pub accent: Hsla,
    pub selection_bg: Hsla,
    pub selection_text: Hsla,
    pub error: Hsla,
    pub success: Hsla,
    pub warning: Hsla,
    /// Dims the window behind a modal dialog.
    pub scrim: Hsla,
    pub is_dark: bool,
}

impl Global for Theme {}

impl Theme {
    pub fn light() -> Self {
        Theme {
            bg: rgb(0xfafafa).into(),
            surface: rgb(0xf0f0f0).into(),
            code_bg: rgb(0xf0f1f3).into(),
            zebra: rgb(0xf4f5f6).into(),
            hover: rgb(0xe9eef7).into(),
            border: rgb(0xd8d8d8).into(),
            text: rgb(0x1f1f1f).into(),
            text_muted: rgb(0x707070).into(),
            accent: rgb(0x2f6feb).into(),
            selection_bg: rgb(0xd6e4ff).into(),
            selection_text: rgb(0x0b2a6b).into(),
            error: rgb(0xc62828).into(),
            success: rgb(0x2e7d32).into(),
            warning: rgb(0xb26a00).into(),
            scrim: gpui::hsla(0., 0., 0., 0.25),
            is_dark: false,
        }
    }

    pub fn dark() -> Self {
        Theme {
            bg: rgb(0x1e1e1e).into(),
            surface: rgb(0x262626).into(),
            code_bg: rgb(0x292b2f).into(),
            zebra: rgb(0x232323).into(),
            hover: rgb(0x2b3038).into(),
            border: rgb(0x3a3a3a).into(),
            text: rgb(0xe6e6e6).into(),
            text_muted: rgb(0x9a9a9a).into(),
            accent: rgb(0x6ea8fe).into(),
            selection_bg: rgb(0x2c4f8a).into(),
            selection_text: rgb(0xffffff).into(),
            error: rgb(0xff6b6b).into(),
            success: rgb(0x6fcf7f).into(),
            warning: rgb(0xf2b84b).into(),
            scrim: gpui::hsla(0., 0., 0., 0.5),
            is_dark: true,
        }
    }

    /// The `ui.theme` preference, falling back to the desktop's appearance.
    pub fn resolve(app: &bibli_core::App, appearance: WindowAppearance) -> Self {
        match app
            .settings
            .get::<String>(crate::settings::THEME_KEY)
            .as_deref()
        {
            Some("light") => Theme::light(),
            Some("dark") => Theme::dark(),
            _ => Theme::for_appearance(appearance),
        }
    }

    pub fn for_appearance(appearance: WindowAppearance) -> Self {
        match appearance {
            WindowAppearance::Light | WindowAppearance::VibrantLight => Theme::light(),
            WindowAppearance::Dark | WindowAppearance::VibrantDark => Theme::dark(),
        }
    }
}
