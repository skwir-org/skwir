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
//!   `success`/`warning`/`error` for outcomes, `scrim` behind a dialog,
//!   `drop_target` under a drag, `on_fill` for text on a saturated fill.
//!   `map_*` draw a map: its graticule, a track, and a route with the
//!   places and areas of a file; land, water and borders are the offline
//!   background's.
//!   `terminal` is the sixteen ANSI colours a terminal's programs name; its
//!   default text and background are `text` and `bg`.
//!   Do not derive a hover from `border` or `surface` locally.
//! - radius: `rounded_sm` inside controls (segments, menu rows, key caps),
//!   `rounded_md` for controls and rows, `rounded_lg` for panels (menus,
//!   toasts, cards, dialogs), `rounded_full` for pills.
//! - elevation: `shadow_md` for menus and popovers, `shadow_lg` for toasts
//!   and dialogs; nothing else casts a shadow.
//! - spacing: multiples of 4 px; rows are 26 px, controls 28 px, bars 42 px
//!   (a pane's row, the sidebar's header, the tab strip: their edges meet).

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
    /// A line or row pointed at without being chosen: the line clicked in
    /// the code view, the setting a link named.
    pub highlight: Hsla,
    /// An edge that is dragged (the preview panel's side, the dock's top)
    /// under the pointer.
    pub handle: Hsla,
    /// Laid over a picture of a page: selected text, and a search match.
    pub page_selection: Hsla,
    pub page_match: Hsla,
    /// A folder, row or place that a drag would drop into.
    pub drop_target: Hsla,
    /// Text and glyphs on a saturated fill (`accent`, `error`, chart blocks).
    pub on_fill: Hsla,
    /// Background of a tag chip.
    pub tag_bg: Hsla,
    /// Where a picture will be once it is loaded.
    pub placeholder: Hsla,
    /// Bars of a chart; the one under the pointer is `accent`.
    pub chart: Hsla,
    /// A map's graticule, on `bg`.
    pub map_grid: Hsla,
    /// A recorded track on a map, and a planned route, places and areas:
    /// told apart at a glance, and from the graticule.
    pub map_track: Hsla,
    pub map_route: Hsla,
    /// What the offline background draws: land, water, and borders.
    pub map_land: Hsla,
    pub map_water: Hsla,
    pub map_border: Hsla,
    /// The ANSI colours, black to white then their bright kin, readable on
    /// `bg`. They are VS Code's defaults for its light and dark themes,
    /// whose backgrounds are close to ours.
    pub terminal: [Hsla; 16],
    pub is_dark: bool,
}

fn palette(hex: [u32; 16]) -> [Hsla; 16] {
    hex.map(|value| rgb(value).into())
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
            highlight: Hsla::from(rgb(0xd6e4ff)).opacity(0.5),
            handle: Hsla::from(rgb(0x2f6feb)).opacity(0.35),
            page_selection: Hsla::from(rgb(0x2f6feb)).opacity(0.35),
            page_match: Hsla::from(rgb(0xffce33)).opacity(0.4),
            drop_target: Hsla::from(rgb(0xd6e4ff)).opacity(0.6),
            on_fill: gpui::white(),
            tag_bg: Hsla::from(rgb(0x2f6feb)).opacity(0.18),
            placeholder: Hsla::from(rgb(0xd8d8d8)).opacity(0.6),
            chart: Hsla::from(rgb(0x2f6feb)).opacity(0.6),
            map_grid: rgb(0xe6e6e6).into(),
            map_track: rgb(0x2f6feb).into(),
            map_route: rgb(0x9333ea).into(),
            map_land: rgb(0xf3f1ec).into(),
            map_water: rgb(0xd5e3f0).into(),
            map_border: rgb(0xb3aaa0).into(),
            terminal: palette([
                0x000000, 0xcd3131, 0x00bc00, 0x949800, 0x0451a5, 0xbc05bc, 0x0598bc, 0x555555,
                0x666666, 0xcd3131, 0x14ce14, 0xb5ba00, 0x0451a5, 0xbc05bc, 0x0598bc, 0xa5a5a5,
            ]),
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
            highlight: Hsla::from(rgb(0x2c4f8a)).opacity(0.5),
            handle: Hsla::from(rgb(0x6ea8fe)).opacity(0.35),
            page_selection: Hsla::from(rgb(0x6ea8fe)).opacity(0.35),
            page_match: Hsla::from(rgb(0xffce33)).opacity(0.4),
            drop_target: Hsla::from(rgb(0x2c4f8a)).opacity(0.6),
            on_fill: gpui::white(),
            tag_bg: Hsla::from(rgb(0x6ea8fe)).opacity(0.18),
            placeholder: Hsla::from(rgb(0x3a3a3a)).opacity(0.6),
            chart: Hsla::from(rgb(0x6ea8fe)).opacity(0.6),
            map_grid: rgb(0x2c2c2c).into(),
            map_track: rgb(0x6ea8fe).into(),
            map_route: rgb(0xc084fc).into(),
            map_land: rgb(0x2b2a27).into(),
            map_water: rgb(0x18212b).into(),
            map_border: rgb(0x57514a).into(),
            terminal: palette([
                0x000000, 0xcd3131, 0x0dbc79, 0xe5e510, 0x2472c8, 0xbc3fbc, 0x11a8cd, 0xe5e5e5,
                0x666666, 0xf14c4c, 0x23d18b, 0xf5f543, 0x3b8eea, 0xd670d6, 0x29b8db, 0xe5e5e5,
            ]),
            is_dark: true,
        }
    }

    /// The fill of block `index` of a chart of folder contents (the Usage
    /// map): folders in shades of the accent, files grey, a selected block
    /// lighter. Neighbours differ in hue so their edges read.
    pub fn block(&self, index: usize, folder: bool, selected: bool) -> Hsla {
        let mut color = if folder { self.accent } else { self.text_muted };
        color.h = (color.h + (index % 7) as f32 * 0.04) % 1.;
        color.s = if folder { 0.55 } else { 0.1 };
        color.l = if selected { 0.65 } else { 0.42 };
        color.a = 1.;
        color
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
