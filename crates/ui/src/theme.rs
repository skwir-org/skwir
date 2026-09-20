//! Theme tokens, chosen from the window appearance and stored as a GPUI global.

use gpui::{Global, Hsla, WindowAppearance, rgb};

/// The color tokens every module renders with.
#[derive(Clone, Debug)]
pub struct Theme {
    pub bg: Hsla,
    pub surface: Hsla,
    /// Background of the row or control under the pointer.
    pub hover: Hsla,
    pub border: Hsla,
    pub text: Hsla,
    pub text_muted: Hsla,
    pub accent: Hsla,
    pub selection_bg: Hsla,
    pub selection_text: Hsla,
    pub error: Hsla,
    pub is_dark: bool,
}

impl Global for Theme {}

impl Theme {
    pub fn light() -> Self {
        Theme {
            bg: rgb(0xfafafa).into(),
            surface: rgb(0xf0f0f0).into(),
            hover: rgb(0xe9eef7).into(),
            border: rgb(0xd8d8d8).into(),
            text: rgb(0x1f1f1f).into(),
            text_muted: rgb(0x707070).into(),
            accent: rgb(0x2f6feb).into(),
            selection_bg: rgb(0xd6e4ff).into(),
            selection_text: rgb(0x0b2a6b).into(),
            error: rgb(0xc62828).into(),
            is_dark: false,
        }
    }

    pub fn dark() -> Self {
        Theme {
            bg: rgb(0x1e1e1e).into(),
            surface: rgb(0x262626).into(),
            hover: rgb(0x2b3038).into(),
            border: rgb(0x3a3a3a).into(),
            text: rgb(0xe6e6e6).into(),
            text_muted: rgb(0x9a9a9a).into(),
            accent: rgb(0x6ea8fe).into(),
            selection_bg: rgb(0x2c4f8a).into(),
            selection_text: rgb(0xffffff).into(),
            error: rgb(0xff6b6b).into(),
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
