//! Theme tokens, chosen from the window appearance and stored as a GPUI global.
//!
//! Every surface draws from these and from the scales below, so a menu, a
//! toast, a dialog and a row agree without anyone comparing hex values:
//!
//! - surfaces: what is read (lists, text, code, tables, bytes, prose) is on
//!   `bg` in `text`; `surface` is chrome (bars, headers, the sidebar, the
//!   letterbox around a picture or a page), told apart from `bg` at
//!   1.08:1 at least (`CHROME`). `code_bg` sets a code block or a
//!   table header apart on `bg`, unless the code theme chosen brings its
//!   own background (`code`, `code_preview::Palette`), `zebra` tints
//!   alternate rows. `picture_backdrop` is behind a picture whose ink,
//!   on nothing, would not read on what it is laid on
//!   (`backdrop::behind`). Code kept in the other look
//!   (`reading.code_appearance`) is drawn with that look's tokens
//!   (`in_look`). `text_muted` is for secondary facts (line numbers,
//!   captions), never for the data.
//! - color: the fields of [`Theme`]. `accent_text` is the accent written
//!   as text (a link, a key, what is chosen on `term`); `accent` marks and
//!   fills. State colors are tokens too: `hover`, laid over whatever is
//!   under the pointer on any ground, `selection_*` for what is chosen,
//!   `success`/`warning`/`error` for outcomes, `scrim` behind a dialog,
//!   `stage` behind a picture, a page or a film on the whole screen,
//!   `find_*` for what a find found in text,
//!   `drop_target` under a drag, `on_accent` for text and glyphs on an
//!   `accent` fill, `handle` for an edge being dragged, `band` and
//!   `band_edge` for the rubber band, `term` behind what the query
//!   understood in the address field (a term's value, the scope's token),
//!   `block_hover` and `block_selected`
//!   for a block of the Usage map pointed at and chosen, over any fill,
//!   `block_text` for what is written on one, `block_rest` for
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
//! - fonts: `font` for the interface, `reading_font` for the running text
//!   of what is read, `code_font` for what is read a character at a time
//!   (`fonts.rs`, ADR-073), resolved with the look; what is drawn names
//!   one of them, never a family, and keeps no copy: a tooltip or a
//!   window of its own reads the theme too.
//! - spacing: multiples of 4 px; rows are 26 px, controls 28 px, bars 42 px
//!   (a pane's row, the sidebar's header, the tab strip: their edges meet).
//!
//! A look is derived (`Theme::derive`, ADR-071) from a few colors, its
//! `Base`: what the desktop's scheme names for that look (Plasma's, a
//! GTK3 theme's: `Desktop::schemes`), else skwir's own (`Base::own`)
//! wearing the desktop's accent; either way in the contrast the desktop
//! asks for (`Desktop`, `Theme::of`, `desktop_style`). A scheme is one
//! look's: the other look is skwir's own, and Automatic takes the
//! scheme's look when it has one look's (`Theme::look`). The other
//! grounds and lines (`zebra`, `code_bg`, `hover`,
//! `border`, a derived selection) are `Steps` in Oklab from `bg` towards
//! `text`, tinted by the accent, `hover` as an overlay that shows its step
//! on `bg` (`color::overlay`); the marks of the accent (`band`,
//! `handle`, `chart`, `term`…) are it, fainter. Then the contrast is
//! guarded: text reads at 4.5:1 on every ground it is drawn on (`hover`
//! laid on `bg` and on `surface`), `accent_text` there and on `term`, the
//! accent at 3:1 on `bg` and `surface` (7:1 and 4.5:1 when the desktop
//! asks for more, `Contrast::More`), a color that does not moved in
//! lightness, its hue kept (`color::readable`); a fill under text (the
//! accent, the selection) gives way when no lightness of its text reads
//! on it. A new
//! token is derived here, or kept (`Kept`) when no desktop's colors
//! change what it means; never a literal beside the others.

use gpui::{Global, Hsla, SharedString, WindowAppearance, rgb};
use skwir_platform::mime::Kind;

use crate::color::{self, Oklab};

/// The color tokens every module renders with.
#[derive(Clone, Debug, PartialEq)]
pub struct Theme {
    pub bg: Hsla,
    pub surface: Hsla,
    /// A code block, an inline code span or a table header, on `bg`.
    pub code_bg: Hsla,
    /// Alternate rows of a table or of a byte dump, on `bg`.
    pub zebra: Hsla,
    /// Laid over the row or control under the pointer, whatever it is
    /// on (`bg`, `surface`): translucent, it darkens or lightens either
    /// by as much (`color::overlay`). Where it must hide what is under
    /// it, it is laid on its ground (`bg.blend(hover)`).
    pub hover: Hsla,
    pub border: Hsla,
    pub text: Hsla,
    pub text_muted: Hsla,
    pub accent: Hsla,
    /// The accent written as text (a link, a term's key, what is chosen
    /// on `term`): it reads as text does wherever text is drawn, and on
    /// `term`. `accent` itself is a mark and a fill, read at 3:1.
    pub accent_text: Hsla,
    pub selection_bg: Hsla,
    pub selection_text: Hsla,
    pub error: Hsla,
    pub success: Hsla,
    pub warning: Hsla,
    /// Dims the window behind a modal dialog.
    pub scrim: Hsla,
    /// Behind a picture, a page or a film shown on the whole screen: black
    /// in either look, as a cinema's or a slide projector's.
    pub stage: Hsla,
    /// A line or row pointed at without being chosen: the line clicked in
    /// the code view, the setting a link named.
    pub highlight: Hsla,
    /// An edge that is dragged (the preview panel's side, the dock's top)
    /// under the pointer.
    pub handle: Hsla,
    /// The rubber band sweeping files: its fill and its edge.
    pub band: Hsla,
    pub band_edge: Hsla,
    /// Behind what the query understood in the address field: a term's
    /// value, the scope's token (ADR-098), the depth chosen. A mark of the
    /// accent, as the band is, a little stronger: it is behind text that
    /// reads on it, `text` or `accent_text`.
    pub term: Hsla,
    /// Laid over a picture of a page: selected text, and a search match.
    pub page_selection: Hsla,
    pub page_match: Hsla,
    /// What a find looks for, wherever text holds it (`find_match`), and
    /// the match it is on (`find_current`, its text `find_current_text`,
    /// dark on it in either look), marked as a browser marks them: a word
    /// found in a long line is seen at a glance.
    pub find_match: Hsla,
    pub find_current: Hsla,
    pub find_current_text: Hsla,
    /// A folder, row or place that a drag would drop into.
    pub drop_target: Hsla,
    /// Text and glyphs on an `accent` fill: white, or dark on a light
    /// accent.
    pub on_accent: Hsla,
    /// What is written on a block of the Usage map, over any of its fills
    /// (`block`, `block_age`, `block_kind`).
    pub block_text: Hsla,
    /// The colors a tag wears, in the order of
    /// `skwir_core::metadata::TAG_COLORS` (red, orange, yellow, green, teal,
    /// blue, purple, pink): a tag's dot is one, its chip `tag_fill` of it.
    pub tags: [Hsla; 8],
    /// A rating's stars.
    pub rating: Hsla,
    /// Where a picture will be once it is loaded.
    pub placeholder: Hsla,
    /// Behind a picture whose ink, on nothing, would not read on what it is
    /// laid on: light, so a drawing's dark lines read on the dark look.
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
    /// What this look took from the desktop, which the other look takes
    /// too (`in_look`).
    pub desktop: Desktop,
    /// The colors code is drawn in and their look: skwir's own colors and
    /// look unless the Reading page chooses (`code_preview::CodeTheme`).
    pub code: crate::code_preview::CodeTheme,
    /// The families skwir writes in, by role (`fonts.rs`, ADR-073): the
    /// interface's, the running text of what is read, what is read a
    /// character at a time. Read where it is drawn, never copied.
    pub font: SharedString,
    pub reading_font: SharedString,
    pub code_font: SharedString,
}

fn palette(hex: [u32; 16]) -> [Hsla; 16] {
    hex.map(|value| rgb(value).into())
}

impl Global for Theme {}

/// The steps of `Theme::block_age`.
pub const AGES: usize = 5;

/// How much contrast the desktop asks for (ADR-071).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Contrast {
    #[default]
    Normal,
    /// More: GNOME's high contrast, macOS's "Increase contrast".
    More,
}

impl Contrast {
    /// What text needs on what it is drawn on, and the accent on the
    /// grounds it marks (WCAG 2: 1.4.3 and 1.4.11, more as 1.4.6).
    fn ratios(self) -> (f32, f32) {
        match self {
            Contrast::Normal => (4.5, 3.),
            Contrast::More => (7., 4.5),
        }
    }
}

/// What a look takes from the desktop (ADR-071, `desktop_style::colors`):
/// its accent, the contrast it asks for, and the scheme the user chose
/// there, a look's each.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Desktop {
    /// The accent a look without a scheme of its own wears.
    pub accent: Option<Hsla>,
    pub contrast: Contrast,
    pub schemes: Schemes,
}

/// The desktop's schemes (Plasma's, a GTK3 theme's), a look's each: what
/// that look is made of in place of skwir's own colors.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Schemes {
    pub light: Option<Base>,
    pub dark: Option<Base>,
}

impl Schemes {
    /// The scheme of the look `dark` names.
    pub fn of(&self, dark: bool) -> Option<&Base> {
        if dark {
            self.dark.as_ref()
        } else {
            self.light.as_ref()
        }
    }

    /// Whether the look is dark when only one look has a scheme: the
    /// scheme's look (Plasma's, a theme without a dark variant), which
    /// `ui.theme`'s Automatic takes. `None` for both or neither.
    pub fn one_look(&self) -> Option<bool> {
        match (self.light, self.dark) {
            (Some(_), None) => Some(false),
            (None, Some(_)) => Some(true),
            _ => None,
        }
    }
}

/// The colors a look is made of: skwir's own (`Base::own`), or what a
/// desktop's scheme names (ADR-071). The other tokens are derived from
/// them (`Theme::derive`), and so is each left `None`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Base {
    pub dark: bool,
    pub bg: Hsla,
    pub surface: Hsla,
    pub text: Hsla,
    pub text_muted: Option<Hsla>,
    pub zebra: Option<Hsla>,
    pub border: Option<Hsla>,
    pub accent: Hsla,
    /// The selection's background and its text.
    pub selection: Option<(Hsla, Hsla)>,
    pub error: Hsla,
    pub warning: Hsla,
    pub success: Hsla,
}

impl Base {
    /// Skwir's own colors, of the dark look or the light one.
    pub fn own(dark: bool) -> Self {
        let [bg, surface, text, accent, error, warning, success] = if dark {
            [
                0x1e1e1e, 0x262626, 0xe6e6e6, 0x6ea8fe, 0xff6b6b, 0xf2b84b, 0x6fcf7f,
            ]
        } else {
            [
                0xfafafa, 0xf0f0f0, 0x1f1f1f, 0x2f6feb, 0xc62828, 0xb26a00, 0x2e7d32,
            ]
        }
        .map(|value| Hsla::from(rgb(value)));
        Base {
            dark,
            bg,
            surface,
            text,
            text_muted: None,
            zebra: None,
            border: None,
            accent,
            selection: None,
            error,
            warning,
            success,
        }
    }
}

/// Where a derived color lies, in Oklab: `along` the way from `bg` (0)
/// to `text` (1), past it beyond 1, its hue moved `tint` of the way to
/// the accent's (0 none, 1 the accent's own).
#[derive(Clone, Copy, Debug)]
struct Step {
    along: f32,
    tint: f32,
}

impl Step {
    const fn new(along: f32, tint: f32) -> Self {
        Step { along, tint }
    }

    fn of(self, bg: Oklab, text: Oklab, accent: Oklab) -> Hsla {
        let at = bg.mix(text, self.along);
        Oklab {
            l: at.l,
            a: at.a + (accent.a - at.a) * self.tint,
            b: at.b + (accent.b - at.b) * self.tint,
        }
        .to_hsla(1.)
    }
}

/// How far the chrome (`surface`) stands from what is read (`bg`) at
/// least: skwir's own looks stand at 1.09:1, a notes app's sidebar from
/// its page at 1.08:1 (Obsidian's, ADR-071's amendment of 2026-10-27).
const CHROME: f32 = 1.08;

/// A look's steps: fitted to the hand-picked palettes they replaced
/// (2026-10-19), which skwir's base, stepped, gives again within two
/// units of a channel (the dark selection within five: it takes the
/// accent's hue). A dark look steps further: its grays are told apart by
/// less.
struct Steps {
    zebra: Step,
    code_bg: Step,
    hover: Step,
    map_grid: Step,
    block_rest: Step,
    border: Step,
    text_muted: Step,
    selection_bg: Step,
    selection_text: Step,
}

const LIGHT: Steps = Steps {
    zebra: Step::new(0.021, 0.01),
    code_bg: Step::new(0.036, 0.015),
    hover: Step::new(0.05, 0.065),
    map_grid: Step::new(0.081, 0.),
    block_rest: Step::new(0.121, 0.),
    border: Step::new(0.138, 0.),
    text_muted: Step::new(0.59, 0.),
    selection_bg: Step::new(0.092, 0.2),
    selection_text: Step::new(0.906, 0.6),
};

const DARK: Steps = Steps {
    zebra: Step::new(0.031, 0.),
    code_bg: Step::new(0.078, 0.057),
    hover: Step::new(0.105, 0.114),
    map_grid: Step::new(0.084, 0.),
    block_rest: Step::new(0.153, 0.),
    border: Step::new(0.164, 0.),
    text_muted: Step::new(0.654, 0.),
    selection_bg: Step::new(0.286, 0.76),
    selection_text: Step::new(1.11, 0.),
};

/// What no desktop's colors change: a color that means something of its
/// own (a tag's, a find's, a map's land and water), or that is drawn on
/// skwir's own fills (the Usage map's) or belongs to no desktop's scheme
/// (a terminal's sixteen).
struct Kept {
    scrim: Hsla,
    stage: Hsla,
    page_match: Hsla,
    find_match: Hsla,
    find_current: Hsla,
    find_current_text: Hsla,
    tags: [Hsla; 8],
    rating: Hsla,
    picture_backdrop: Hsla,
    block_text: Hsla,
    block_hover: Hsla,
    block_selected: Hsla,
    block_reclaim: Hsla,
    map_route: Hsla,
    map_land: Hsla,
    map_water: Hsla,
    map_border: Hsla,
    terminal: [Hsla; 16],
}

impl Kept {
    fn light() -> Self {
        Kept {
            scrim: gpui::hsla(0., 0., 0., 0.25),
            stage: gpui::black(),
            page_match: Hsla::from(rgb(0xffce33)).opacity(0.4),
            find_match: Hsla::from(rgb(0xffce33)).opacity(0.5),
            find_current: rgb(0xff9632).into(),
            find_current_text: rgb(0x1f1f1f).into(),
            tags: [
                0xd93025, 0xe8710a, 0xc49a00, 0x1e8e3e, 0x00897b, 0x2f6feb, 0x8e44d8, 0xd6336c,
            ]
            .map(|value| rgb(value).into()),
            rating: rgb(0xe0a100).into(),
            picture_backdrop: gpui::white(),
            block_text: gpui::white(),
            block_hover: gpui::white().opacity(0.2),
            block_selected: rgb(0x1f1f1f).into(),
            block_reclaim: gpui::white().opacity(0.85),
            map_route: rgb(0x9333ea).into(),
            map_land: rgb(0xf3f1ec).into(),
            map_water: rgb(0xd5e3f0).into(),
            map_border: rgb(0xb3aaa0).into(),
            terminal: palette([
                0x000000, 0xcd3131, 0x00bc00, 0x949800, 0x0451a5, 0xbc05bc, 0x0598bc, 0x555555,
                0x666666, 0xcd3131, 0x14ce14, 0xb5ba00, 0x0451a5, 0xbc05bc, 0x0598bc, 0xa5a5a5,
            ]),
        }
    }

    fn dark() -> Self {
        Kept {
            scrim: gpui::hsla(0., 0., 0., 0.5),
            stage: gpui::black(),
            page_match: Hsla::from(rgb(0xffce33)).opacity(0.4),
            find_match: Hsla::from(rgb(0xffce33)).opacity(0.35),
            find_current: rgb(0xff9632).into(),
            find_current_text: rgb(0x1f1f1f).into(),
            tags: [
                0xff7b72, 0xffa657, 0xe3c55b, 0x7ee787, 0x56d4c4, 0x6ea8fe, 0xc49cff, 0xff8fb3,
            ]
            .map(|value| rgb(value).into()),
            rating: rgb(0xf2c14b).into(),
            picture_backdrop: rgb(0xd8d8d8).into(),
            block_text: gpui::white(),
            block_hover: gpui::white().opacity(0.16),
            block_selected: gpui::white(),
            block_reclaim: gpui::white().opacity(0.75),
            map_route: rgb(0xc084fc).into(),
            map_land: rgb(0x2b2a27).into(),
            map_water: rgb(0x18212b).into(),
            map_border: rgb(0x57514a).into(),
            terminal: palette([
                0x000000, 0xcd3131, 0x0dbc79, 0xe5e510, 0x2472c8, 0xbc3fbc, 0x11a8cd, 0xe5e5e5,
                0x666666, 0xf14c4c, 0x23d18b, 0xf5f543, 0x3b8eea, 0xd670d6, 0x29b8db, 0xe5e5e5,
            ]),
        }
    }
}

/// `fg` made to read at `ratio` on each of `grounds` (`color::readable`),
/// said when even black or white cannot.
fn guarded(name: &str, fg: Hsla, grounds: &[Hsla], ratio: f32) -> Hsla {
    let fg = color::readable(fg, grounds, ratio);
    if !color::reads(fg, grounds, ratio) {
        tracing::debug!("theme: {name} cannot read at {ratio}:1 on all its grounds");
    }
    fg
}

/// Skwir's own looks, derived once: code kept in the other look asks for
/// one as it is drawn (`Theme::in_look`).
static OWN: std::sync::LazyLock<[Theme; 2]> = std::sync::LazyLock::new(|| {
    [false, true].map(|dark| Theme::derive(&Base::own(dark), Contrast::Normal))
});

impl Theme {
    pub fn light() -> Self {
        OWN[0].clone()
    }

    pub fn dark() -> Self {
        OWN[1].clone()
    }

    /// Every token of the look `base` makes, its contrast guarded.
    pub fn derive(base: &Base, contrast: Contrast) -> Self {
        let (text_ratio, mark_ratio) = contrast.ratios();
        let steps = if base.dark { &DARK } else { &LIGHT };
        let kept = if base.dark {
            Kept::dark()
        } else {
            Kept::light()
        };
        let bg = base.bg;
        // The chrome stands apart from what is read, at least as far as a
        // notes app's sidebar from its page: a scheme whose window is as
        // light as its view (a GTK theme's) has it moved towards its text
        // or away, whichever is nearer, its hue kept.
        let surface = color::readable(base.surface, &[bg], CHROME);
        // The accent marks `bg` and `surface` (a focus ring, a switch that
        // is on) and is a fill under `on_accent`: white, or the darkest of
        // the look where that reads better.
        let accent = guarded("accent", base.accent, &[bg, surface], mark_ratio);
        let darkest = if color::luminance(base.text) < color::luminance(bg) {
            base.text
        } else {
            bg
        };
        let on = if color::ratio(gpui::white(), accent) >= color::ratio(darkest, accent) {
            gpui::white()
        } else {
            darkest
        };
        let (on_accent, accent) = color::readable_on(on, accent, text_ratio);

        let (from, to, hue) = (Oklab::of(bg), Oklab::of(base.text), Oklab::of(accent));
        let step = |step: Step| step.of(from, to, hue);
        let zebra = base.zebra.unwrap_or_else(|| step(steps.zebra));
        let code_bg = step(steps.code_bg);
        let hover = color::overlay(bg, step(steps.hover));
        let term = accent.opacity(0.18);
        let border = base.border.unwrap_or_else(|| step(steps.border));
        let border = match contrast {
            Contrast::Normal => border,
            Contrast::More => guarded("border", border, &[bg, surface], 3.),
        };
        let (selection_bg, selection_text) = base
            .selection
            .unwrap_or_else(|| (step(steps.selection_bg), step(steps.selection_text)));
        let (selection_text, selection_bg) =
            color::readable_on(selection_text, selection_bg, text_ratio);
        let grounds = [
            bg,
            surface,
            code_bg,
            zebra,
            bg.blend(hover),
            surface.blend(hover),
        ];
        let text = guarded("text", base.text, &grounds, text_ratio);
        let text_muted = base.text_muted.unwrap_or_else(|| step(steps.text_muted));
        let text_muted = guarded("text_muted", text_muted, &grounds, text_ratio);
        let termed = [grounds.as_slice(), &[bg.blend(term), surface.blend(term)]].concat();
        let accent_text = guarded("accent_text", accent, &termed, text_ratio);
        let outcome = |name, color| guarded(name, color, &[bg, surface], text_ratio);

        Theme {
            bg,
            surface,
            code_bg,
            zebra,
            hover,
            border,
            text,
            text_muted,
            accent,
            accent_text,
            selection_bg,
            selection_text,
            error: outcome("error", base.error),
            success: outcome("success", base.success),
            warning: outcome("warning", base.warning),
            scrim: kept.scrim,
            stage: kept.stage,
            highlight: selection_bg.opacity(0.5),
            handle: accent.opacity(0.35),
            band: accent.opacity(0.14),
            band_edge: accent.opacity(0.8),
            term,
            page_selection: accent.opacity(0.35),
            page_match: kept.page_match,
            find_match: kept.find_match,
            find_current: kept.find_current,
            find_current_text: kept.find_current_text,
            drop_target: selection_bg.opacity(0.6),
            on_accent,
            block_text: kept.block_text,
            tags: kept.tags,
            rating: kept.rating,
            placeholder: border.opacity(0.6),
            picture_backdrop: kept.picture_backdrop,
            chart: accent.opacity(0.6),
            block_hover: kept.block_hover,
            block_selected: kept.block_selected,
            block_reclaim: kept.block_reclaim,
            block_rest: step(steps.block_rest),
            map_grid: step(steps.map_grid),
            map_track: accent,
            map_route: kept.map_route,
            map_land: kept.map_land,
            map_water: kept.map_water,
            map_border: kept.map_border,
            terminal: kept.terminal,
            is_dark: base.dark,
            desktop: Desktop {
                contrast,
                ..Desktop::default()
            },
            code: crate::code_preview::CodeTheme::own(base.dark),
            font: crate::fonts::Families::default().ui,
            reading_font: crate::fonts::Families::default().reading,
            code_font: crate::fonts::Families::default().code,
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

    /// The fill of a tag's chip laid over a picture (a tile's): its
    /// `tag_fill` on `bg`, opaque, so it reads on whatever is under it.
    pub fn tag_solid(&self, index: usize) -> Hsla {
        self.bg.blend(self.tag_fill(index))
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
        let mut other = Theme::of(dark, self.desktop);
        other.code = self.code.clone();
        other.set_fonts(self.families());
        std::borrow::Cow::Owned(other)
    }

    /// The look `dark` names, wearing what it takes from the desktop:
    /// skwir's own when that is nothing, else derived once and kept while
    /// the desktop says the same (`in_look` asks as it draws).
    pub fn of(dark: bool, desktop: Desktop) -> Self {
        static MADE: std::sync::Mutex<Vec<Theme>> = std::sync::Mutex::new(Vec::new());
        if desktop == Desktop::default() {
            return if dark { Theme::dark() } else { Theme::light() };
        }
        let mut made = MADE
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if let Some(theme) = made
            .iter()
            .find(|theme| theme.is_dark == dark && theme.desktop == desktop)
        {
            return theme.clone();
        }
        // The desktop's scheme of this look, else skwir's own wearing the
        // desktop's accent.
        let base = desktop.schemes.of(dark).copied().unwrap_or_else(|| Base {
            accent: desktop.accent.unwrap_or(Base::own(dark).accent),
            ..Base::own(dark)
        });
        let mut theme = Theme::derive(&base, desktop.contrast);
        theme.desktop = desktop;
        // The two looks of what the desktop says now.
        if made.len() == 2 {
            made.remove(0);
        }
        made.push(theme.clone());
        theme
    }

    /// Whether the look is dark: as `ui.theme` says (`chosen`), Automatic
    /// taking the look of the desktop's scheme when it has one look's (a
    /// scheme is that look's: the other look is skwir's own, ADR-071),
    /// else the desktop's `appearance`.
    pub fn look(chosen: Option<&str>, desktop: &Desktop, appearance: WindowAppearance) -> bool {
        match chosen {
            Some("light") => false,
            Some("dark") => true,
            _ => desktop.schemes.one_look().unwrap_or(matches!(
                appearance,
                WindowAppearance::Dark | WindowAppearance::VibrantDark
            )),
        }
    }

    /// The families it writes in.
    pub fn families(&self) -> crate::fonts::Families {
        crate::fonts::Families {
            ui: self.font.clone(),
            reading: self.reading_font.clone(),
            code: self.code_font.clone(),
        }
    }

    pub fn set_fonts(&mut self, families: crate::fonts::Families) {
        self.font = families.ui;
        self.reading_font = families.reading;
        self.code_font = families.code;
    }

    /// The `ui.theme` preference, else the look of the desktop's scheme
    /// when it has one look's, else the desktop's appearance; in what the
    /// desktop's style lends it (`desktop_style::colors`), and its fonts
    /// as `text` draws them (`fonts::resolve`).
    pub fn resolve(
        app: &skwir_core::App,
        appearance: WindowAppearance,
        text: &gpui::TextSystem,
    ) -> Self {
        let desktop = crate::desktop_style::colors(&app.settings);
        let dark = Self::look(
            app.settings
                .get::<String>(crate::settings::THEME_KEY)
                .as_deref(),
            &desktop,
            appearance,
        );
        let mut theme = Theme::of(dark, desktop);
        theme.code = crate::code_preview::CodeTheme::chosen(app, theme.is_dark);
        theme.set_fonts(crate::fonts::resolve(app, text));
        theme
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A color, what it is drawn on, the ratio it must read at there, and
    /// whether those grounds are fills that give way (`readable_on`).
    struct Group {
        name: &'static str,
        fg: Hsla,
        grounds: Vec<Hsla>,
        ratio: f32,
        fill: bool,
    }

    fn groups(t: &Theme, contrast: Contrast) -> Vec<Group> {
        let (text, mark) = contrast.ratios();
        let grounds = vec![
            t.bg,
            t.surface,
            t.code_bg,
            t.zebra,
            t.bg.blend(t.hover),
            t.surface.blend(t.hover),
        ];
        let termed = [
            grounds.as_slice(),
            &[t.bg.blend(t.term), t.surface.blend(t.term)],
        ]
        .concat();
        let on = |name, fg, grounds: &[Hsla], ratio, fill| Group {
            name,
            fg,
            grounds: grounds.to_vec(),
            ratio,
            fill,
        };
        let mut groups = vec![
            on("text", t.text, &grounds, text, false),
            on("text_muted", t.text_muted, &grounds, text, false),
            on(
                "selection_text",
                t.selection_text,
                &[t.selection_bg],
                text,
                true,
            ),
            on("on_accent", t.on_accent, &[t.accent], text, true),
            on("error", t.error, &[t.bg, t.surface], text, false),
            on("warning", t.warning, &[t.bg, t.surface], text, false),
            on("success", t.success, &[t.bg, t.surface], text, false),
            on("accent", t.accent, &[t.bg, t.surface], mark, false),
            on("accent_text", t.accent_text, &termed, text, false),
            on("surface", t.surface, &[t.bg], CHROME, false),
        ];
        if contrast == Contrast::More {
            groups.push(on("border", t.border, &[t.bg, t.surface], 3., false));
        }
        groups
    }

    #[test]
    fn skwir_s_looks_read_in_either_contrast() {
        for dark in [false, true] {
            for contrast in [Contrast::Normal, Contrast::More] {
                let theme = Theme::derive(&Base::own(dark), contrast);
                for g in groups(&theme, contrast) {
                    assert!(
                        color::reads(g.fg, &g.grounds, g.ratio),
                        "{} in the {} look, {contrast:?}",
                        g.name,
                        if dark { "dark" } else { "light" }
                    );
                }
            }
        }
    }

    #[test]
    fn any_scheme_reads_wherever_a_color_could() {
        // A scheme of any colors, a light one or a dark one: each color
        // reads on its grounds, unless no color could (a ground of middle
        // gray under text asked to read at 7:1); a fill always gives way.
        let mut seed = 0x2545_f491_u32;
        let mut next = move |low: u32, high: u32| {
            seed ^= seed << 13;
            seed ^= seed >> 17;
            seed ^= seed << 5;
            low + seed % (high - low + 1)
        };
        let mut gray = |low, high| {
            let [r, g, b] = [(); 3].map(|()| next(low, high));
            Hsla::from(rgb(r << 16 | g << 8 | b))
        };
        for case in 0..400 {
            let dark = case % 2 == 1;
            let (ground, ink) = if dark {
                ((0, 90), (120, 255))
            } else {
                ((170, 255), (0, 140))
            };
            let bg = gray(ground.0, ground.1);
            let base = Base {
                dark,
                bg,
                surface: gray(ground.0, ground.1),
                text: gray(ink.0, ink.1),
                text_muted: (case % 3 == 0).then(|| gray(0, 255)),
                zebra: None,
                border: (case % 5 == 0).then(|| gray(0, 255)),
                accent: gray(0, 255),
                selection: (case % 4 == 0).then(|| (gray(0, 255), gray(0, 255))),
                error: gray(0, 255),
                warning: gray(0, 255),
                success: gray(0, 255),
            };
            for contrast in [Contrast::Normal, Contrast::More] {
                let theme = Theme::derive(&base, contrast);
                for g in groups(&theme, contrast) {
                    let could = [gpui::black(), gpui::white()]
                        .iter()
                        .any(|c| color::reads(*c, &g.grounds, g.ratio));
                    assert!(
                        color::reads(g.fg, &g.grounds, g.ratio) || (!g.fill && !could),
                        "case {case}, {}, {contrast:?}: {base:?}",
                        g.name
                    );
                }
            }
        }
    }

    #[test]
    fn skwir_s_steps_give_its_hand_picked_colors_again() {
        let near = |a: Hsla, b: u32, units: f32| {
            let (a, b) = (a.to_rgb(), rgb(b));
            [a.r - b.r, a.g - b.g, a.b - b.b]
                .iter()
                .all(|d| d.abs() * 255. <= units)
        };
        let light = Theme::light();
        for (token, was) in [
            (light.zebra, 0xf4f5f6),
            (light.code_bg, 0xf0f1f3),
            (light.bg.blend(light.hover), 0xe9eef7),
            (light.border, 0xd8d8d8),
            (light.block_rest, 0xdcdcdc),
            (light.map_grid, 0xe6e6e6),
            (light.selection_bg, 0xd6e4ff),
            (light.selection_text, 0x0b2a6b),
        ] {
            assert!(near(token, was, 2.5), "{was:06x}: {:?}", token.to_rgb());
        }
        let dark = Theme::dark();
        for (token, was) in [
            (dark.zebra, 0x232323),
            (dark.code_bg, 0x292b2f),
            (dark.bg.blend(dark.hover), 0x2b3038),
            (dark.border, 0x3a3a3a),
            (dark.block_rest, 0x383838),
            (dark.map_grid, 0x2c2c2c),
            // A little lighter than picked (0x9a9a9a): it reads on the
            // hover laid on the sidebar too (2026-10-27).
            (dark.text_muted, 0x9e9e9e),
            (dark.selection_text, 0xffffff),
        ] {
            assert!(near(token, was, 2.5), "{was:06x}: {:?}", token.to_rgb());
        }
        // The selection takes the accent's hue, a little bluer than it was.
        assert!(near(dark.selection_bg, 0x2c4f8a, 5.));
    }

    #[test]
    fn real_schemes_read_in_either_contrast() {
        // As read 2026-10-20 from Breeze 6.7.5's schemes, adw-gtk3 6.5 and
        // Materia 20210322 (`kde_scheme`, `gtk_theme`): bg, surface, text,
        // text_muted, zebra, border, accent, selection, error, warning,
        // success; 0 where the scheme says none.
        let schemes: [(&str, [u32; 12]); 6] = [
            (
                "Breeze Light",
                [
                    0xffffff, 0xeff0f1, 0x232629, 0x707d8a, 0xf7f7f7, 0, 0x3daee9, 0x3daee9,
                    0xffffff, 0xda4453, 0xf67400, 0x27ae60,
                ],
            ),
            (
                "Breeze Dark",
                [
                    0x141618, 0x202326, 0xfcfcfc, 0xa1a9b1, 0x1d1f22, 0, 0x3daee9, 0x3daee9,
                    0xfcfcfc, 0xda4453, 0xf67400, 0x27ae60,
                ],
            ),
            (
                "adw-gtk3",
                [
                    0xffffff, 0xfafafb, 0x333334, 0, 0, 0, 0x3584e4, 0x3584e4, 0xffffff, 0xba161e,
                    0xbe8908, 0x26a169,
                ],
            ),
            (
                "adw-gtk3 dark",
                [
                    0x1d1d20, 0x222226, 0xffffff, 0, 0, 0, 0x3584e4, 0x3584e4, 0xffffff, 0xd9777e,
                    0xefc96c, 0x7dc7a5,
                ],
            ),
            (
                "Materia",
                [
                    0xffffff, 0xf9f9f9, 0x212121, 0, 0, 0xe0e0e0, 0x1967d2, 0x1967d2, 0xffffff,
                    0xd93025, 0xf9ab00, 0x1e8e3e,
                ],
            ),
            (
                "Materia dark",
                [
                    0x1e1e1e, 0x121212, 0xe2e2e2, 0, 0, 0x393939, 0x8ab4f8, 0x8ab4f8, 0x000000,
                    0xf28b82, 0xfdd663, 0x81c995,
                ],
            ),
        ];
        for (
            name,
            [
                bg,
                surface,
                text,
                muted,
                zebra,
                border,
                accent,
                sel_bg,
                sel_text,
                error,
                warning,
                success,
            ],
        ) in schemes
        {
            let color = |value: u32| Hsla::from(rgb(value));
            let some = |value: u32| (value != 0).then(|| color(value));
            let base = Base {
                dark: name.contains("Dark") || name.contains("dark"),
                bg: color(bg),
                surface: color(surface),
                text: color(text),
                text_muted: some(muted),
                zebra: some(zebra),
                border: some(border),
                accent: color(accent),
                selection: Some((color(sel_bg), color(sel_text))),
                error: color(error),
                warning: color(warning),
                success: color(success),
            };
            for contrast in [Contrast::Normal, Contrast::More] {
                let theme = Theme::derive(&base, contrast);
                for g in groups(&theme, contrast) {
                    assert!(
                        color::reads(g.fg, &g.grounds, g.ratio),
                        "{} of {name}, {contrast:?}",
                        g.name
                    );
                }
            }
        }
    }

    #[test]
    fn a_scheme_s_own_colors_are_kept_where_they_read() {
        let muted = Hsla::from(rgb(0x555555));
        let selection = (Hsla::from(rgb(0x3daee9)), Hsla::from(rgb(0x000000)));
        let base = Base {
            text_muted: Some(muted),
            selection: Some(selection),
            ..Base::own(false)
        };
        let theme = Theme::derive(&base, Contrast::Normal);
        assert_eq!(theme.text_muted, muted);
        assert_eq!((theme.selection_bg, theme.selection_text), selection);
    }

    #[test]
    fn the_three_pairs_that_did_not_read_now_do() {
        // 2026-10-19: muted text on the light sidebar (4.35:1), a warning
        // on the light look (4.06:1), white on the dark look's accent
        // (2.42:1).
        let (light, dark) = (Theme::light(), Theme::dark());
        assert!(color::ratio(light.text_muted, light.surface) >= 4.5);
        assert!(color::ratio(light.warning, light.bg) >= 4.5);
        assert!(color::ratio(dark.on_accent, dark.accent) >= 4.5);
        assert!(color::luminance(dark.on_accent) < color::luminance(dark.accent));
        assert_eq!(light.on_accent, gpui::white());
    }
}
