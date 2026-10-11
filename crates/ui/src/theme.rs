//! Theme tokens, chosen from the window appearance and stored as a GPUI global.
//!
//! Every surface draws from these and from the scales below, so a menu, a
//! toast, a dialog and a row agree without anyone comparing hex values:
//!
//! - surfaces: what is read (lists, text, code, tables, bytes, prose) is on
//!   `bg` in `text`; `surface` is chrome (bars, headers, the sidebar, the
//!   letterbox around a picture or a page). `code_bg` sets a code block or a
//!   table header apart on `bg`, unless the code theme chosen brings its
//!   own background (`code`, `code_preview::Palette`), `zebra` tints
//!   alternate rows. `picture_backdrop` is behind a thumbnail whose ink,
//!   on nothing, would not read on `bg` (`thumbs::Thumb::on_backdrop`). Code kept in the other look
//!   (`reading.code_appearance`) is drawn with that look's tokens
//!   (`in_look`). `text_muted` is for secondary facts (line numbers,
//!   captions), never for the data.
//! - color: the fields of [`Theme`]. State colors are tokens too: `hover` for
//!   whatever is under the pointer, `selection_*` for what is chosen,
//!   `success`/`warning`/`error` for outcomes, `scrim` behind a dialog,
//!   `drop_target` under a drag, `on_fill` for text on a saturated fill,
//!   `handle` for an edge being dragged, `band` and `band_edge` for the
//!   rubber band, `block_hover` and `block_selected` for a block of the
//!   Usage map pointed at and chosen, over any fill, `block_rest` for
//!   a block that stands for several entries (the smaller ones, the
//!   hidden ones), and `block_reclaim` for the line around a block whose
//!   room can be had back (a cache, a trash); its fills are `block`,
//!   `block_age` and `block_kind`.
//!   `map_*` draw a map: its graticule, a track, and a route with the
//!   places and areas of a file; land, water and borders are the offline
//!   background's.
//!   `tags` are the colors a tag can wear, `rating` a rating's stars.
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
use skwir_platform::mime::Kind;

/// The color tokens every module renders with.
#[derive(Clone, Debug, PartialEq)]
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
    /// The rubber band sweeping files: its fill and its edge.
    pub band: Hsla,
    pub band_edge: Hsla,
    /// Laid over a picture of a page: selected text, and a search match.
    pub page_selection: Hsla,
    pub page_match: Hsla,
    /// A folder, row or place that a drag would drop into.
    pub drop_target: Hsla,
    /// Text and glyphs on a saturated fill (`accent`, `error`, chart blocks).
    pub on_fill: Hsla,
    /// The colors a tag wears, in the order of
    /// `skwir_core::metadata::TAG_COLORS` (red, orange, yellow, green, teal,
    /// blue, purple, pink): a tag's dot is one, its chip `tag_fill` of it.
    pub tags: [Hsla; 8],
    /// A rating's stars.
    pub rating: Hsla,
    /// Where a picture will be once it is loaded.
    pub placeholder: Hsla,
    /// Behind a thumbnail whose ink, on nothing, would not read on `bg`:
    /// light, so a drawing's dark lines read on the dark look.
    pub picture_backdrop: Hsla,
    /// Bars of a chart; the one under the pointer is `accent`.
    pub chart: Hsla,
    /// Laid over the Usage map's block under the pointer.
    pub block_hover: Hsla,
    /// The ring around a chosen block of the Usage map.
    pub block_selected: Hsla,
    /// The dashed line inside a block of the Usage map whose room can be
    /// had back, on any fill.
    pub block_reclaim: Hsla,
    /// A block of the Usage map that stands for several entries, under
    /// text in `text`.
    pub block_rest: Hsla,
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
    /// The colors code is drawn in and their look: skwir's own colors and
    /// look unless the Reading page chooses (`code_preview::CodeTheme`).
    pub code: crate::code_preview::CodeTheme,
}

fn palette(hex: [u32; 16]) -> [Hsla; 16] {
    hex.map(|value| rgb(value).into())
}

impl Global for Theme {}

/// The steps of `Theme::block_age`.
pub const AGES: usize = 5;

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
            band: Hsla::from(rgb(0x2f6feb)).opacity(0.14),
            band_edge: Hsla::from(rgb(0x2f6feb)).opacity(0.8),
            page_selection: Hsla::from(rgb(0x2f6feb)).opacity(0.35),
            page_match: Hsla::from(rgb(0xffce33)).opacity(0.4),
            drop_target: Hsla::from(rgb(0xd6e4ff)).opacity(0.6),
            on_fill: gpui::white(),
            tags: [
                0xd93025, 0xe8710a, 0xc49a00, 0x1e8e3e, 0x00897b, 0x2f6feb, 0x8e44d8, 0xd6336c,
            ]
            .map(|value| rgb(value).into()),
            rating: rgb(0xe0a100).into(),
            placeholder: Hsla::from(rgb(0xd8d8d8)).opacity(0.6),
            picture_backdrop: gpui::white(),
            chart: Hsla::from(rgb(0x2f6feb)).opacity(0.6),
            block_hover: gpui::white().opacity(0.2),
            block_selected: rgb(0x1f1f1f).into(),
            block_reclaim: gpui::white().opacity(0.85),
            block_rest: rgb(0xdcdcdc).into(),
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
            code: crate::code_preview::CodeTheme::own(false),
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
            band: Hsla::from(rgb(0x6ea8fe)).opacity(0.14),
            band_edge: Hsla::from(rgb(0x6ea8fe)).opacity(0.8),
            page_selection: Hsla::from(rgb(0x6ea8fe)).opacity(0.35),
            page_match: Hsla::from(rgb(0xffce33)).opacity(0.4),
            drop_target: Hsla::from(rgb(0x2c4f8a)).opacity(0.6),
            on_fill: gpui::white(),
            tags: [
                0xff7b72, 0xffa657, 0xe3c55b, 0x7ee787, 0x56d4c4, 0x6ea8fe, 0xc49cff, 0xff8fb3,
            ]
            .map(|value| rgb(value).into()),
            rating: rgb(0xf2c14b).into(),
            placeholder: Hsla::from(rgb(0x3a3a3a)).opacity(0.6),
            picture_backdrop: rgb(0xd8d8d8).into(),
            chart: Hsla::from(rgb(0x6ea8fe)).opacity(0.6),
            block_hover: gpui::white().opacity(0.16),
            block_selected: gpui::white(),
            block_reclaim: gpui::white().opacity(0.75),
            block_rest: rgb(0x383838).into(),
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
            code: crate::code_preview::CodeTheme::own(true),
        }
    }

    /// The fill of block `index` of a chart of folder contents (the Usage
    /// map), `depth` folders inside the one shown: folders in shades of
    /// the accent, files a greyer shade of the same, lighter the deeper
    /// they lie. Neighbours differ in hue so their edges read; what is
    /// inside a block keeps its hue (`index` is the outer block's).
    pub fn block(&self, index: usize, folder: bool, depth: usize) -> Hsla {
        let mut color = self.accent;
        color.h = (color.h + (index % 7) as f32 * 0.04) % 1.;
        color.s = if folder { 0.55 } else { 0.18 };
        color.l = 0.40 + depth.min(3) as f32 * 0.035;
        color.a = 1.;
        color
    }

    /// The fill of a block of the Usage map changed `age` steps ago
    /// (`AGES`: within a day, a week, a month, a year, longer): warm and
    /// bright when new, a grey slate when old, lighter the deeper it lies.
    pub fn block_age(&self, age: usize, depth: usize) -> Hsla {
        const RAMP: [(f32, f32); AGES] = [
            (0.07, 0.80),
            (0.10, 0.62),
            (0.14, 0.45),
            (0.55, 0.22),
            (0.60, 0.06),
        ];
        let (h, s) = RAMP[age.min(AGES - 1)];
        gpui::hsla(h, s, 0.40 + depth.min(3) as f32 * 0.035, 1.)
    }

    /// The fill of a block of the Usage map whose files are mostly of
    /// `kind` (`None`: of no kind the names say), `share` of its bytes:
    /// a file wears its kind whole, a folder of mixed files greyer.
    pub fn block_kind(&self, kind: Option<Kind>, share: f32, depth: usize) -> Hsla {
        // Each hue, and how much darker it is drawn: green, cyan and
        // amber look lighter than blue at one lightness, and white text
        // must read on all of them.
        let (hue, darker) = match kind {
            Some(Kind::Image) => (0.39, 0.10),
            Some(Kind::Video) => (0.92, 0.),
            Some(Kind::Audio) => (0.07, 0.04),
            Some(Kind::Document) => (0.60, 0.),
            Some(Kind::Spreadsheet) => (0.45, 0.08),
            Some(Kind::Presentation) => (0.12, 0.08),
            Some(Kind::Text) => (0.74, 0.),
            Some(Kind::Archive) => (0.03, 0.),
            Some(Kind::Font) => (0.83, 0.),
            Some(Kind::Program) => (0.53, 0.08),
            Some(Kind::Folder) | None => (0., 0.),
        };
        let s = if kind.is_none_or(|k| k == Kind::Folder) {
            0.
        } else {
            0.12 + 0.48 * share.clamp(0., 1.)
        };
        gpui::hsla(hue, s, 0.40 - darker + depth.min(3) as f32 * 0.035, 1.)
    }

    /// The tag color of palette index `index` (`metadata::TAG_COLORS`).
    pub fn tag(&self, index: usize) -> Hsla {
        self.tags[index % self.tags.len()]
    }

    /// The fill of a chip of a tag wearing color `index` (see `tags`),
    /// under text in `text`.
    pub fn tag_fill(&self, index: usize) -> Hsla {
        let color = self.tag(index);
        color.opacity(if self.is_dark { 0.3 } else { 0.18 })
    }

    /// The fill of the chip of the tag a pane shows: its pill in the
    /// sidebar, the last of the address's trail.
    pub fn tag_shown(&self, index: usize) -> Hsla {
        self.tag(index).opacity(0.45)
    }

    /// The tokens of the look `dark` names: these, or the other look's,
    /// which code kept in it is drawn with (`reading.code_appearance`): its
    /// gutter, its selection, the background skwir's own colors are on.
    pub fn in_look(&self, dark: bool) -> std::borrow::Cow<'_, Theme> {
        if dark == self.is_dark {
            return std::borrow::Cow::Borrowed(self);
        }
        let mut other = if dark { Theme::dark() } else { Theme::light() };
        other.code = self.code.clone();
        std::borrow::Cow::Owned(other)
    }

    /// The `ui.theme` preference, falling back to the desktop's appearance.
    pub fn resolve(app: &skwir_core::App, appearance: WindowAppearance) -> Self {
        let mut theme = match app
            .settings
            .get::<String>(crate::settings::THEME_KEY)
            .as_deref()
        {
            Some("light") => Theme::light(),
            Some("dark") => Theme::dark(),
            _ => Theme::for_appearance(appearance),
        };
        theme.code = crate::code_preview::CodeTheme::chosen(app, theme.is_dark);
        theme
    }

    pub fn for_appearance(appearance: WindowAppearance) -> Self {
        match appearance {
            WindowAppearance::Light | WindowAppearance::VibrantLight => Theme::light(),
            WindowAppearance::Dark | WindowAppearance::VibrantDark => Theme::dark(),
        }
    }
}
