//! Text sizes (ADR-117): each part of the window is written at a size of
//! its own, and what holds text is as tall as that text needs.
//!
//! - **Parts.** The interface (bars, menus, the sidebar, dialogs), the
//!   files (the views of a pane: names and details in the list, the grid,
//!   the columns and the board), reading (the running text of previews)
//!   and code. Each has a setting in steps (`STEPS`), a percentage of its
//!   100 %; the files' 100 % is the interface's size.
//! - **A size is a part's body text**, in pixels: Skwir's own at 100 %
//!   (`Part::base`) times the setting.
//! - **A part is drawn in a rem of its own** (`Sizes::rem`, the
//!   `elements::rem` element): gpui's `text_sm` is its body text there,
//!   `text_xs` its small text, and every length written in rems (the
//!   spacing scale, `p_1`, `gap_2`) follows it.
//! - **Lengths written at 100 %** (a row's 26 px, a column's width) are
//!   scaled where they are drawn, in whole pixels (`Scale::px`): by the
//!   theme's scale for the part in `render` (`Sizes::scale`), by the rem
//!   in force in an element's layout (`Scale::of`). Pictures and icons
//!   keep the zoom's size: only what holds text grows with it.

use gpui::{Pixels, Window, px};
use skwir_core::App;

/// The steps a part's size is chosen in, as percentages of its 100 %.
pub const STEPS: [u32; 8] = [80, 90, 100, 110, 125, 150, 175, 200];
/// The steps of the preview's own size buttons, which go further, for the
/// file shown (`inspector.rs`).
pub const PREVIEW_STEPS: [u32; 11] = [60, 70, 80, 90, 100, 110, 125, 150, 175, 200, 250];
/// A part at its own size.
pub const DEFAULT_STEP: u32 = 100;

/// gpui's rem at 100 %, against which the spacing scale is written: its
/// `text_sm` (0.875 rem) is Skwir's 14 px body text there.
pub const REM: f32 = 16.;
/// The share of a rem that is a part's body text (gpui's `text_sm`).
const BODY_REMS: f32 = 0.875;

/// A part of the window with a text size of its own.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Part {
    /// Bars, menus, the sidebar, dialogs, tooltips, footers.
    Interface,
    /// The views of a pane: names and details in the list, the grid, the
    /// columns and the board.
    Files,
    /// The running text of previews: Markdown, HTML, documents, books,
    /// mail.
    Reading,
    /// Code previews, the byte view, the terminal.
    Code,
}

impl Part {
    pub const ALL: [Part; 4] = [Part::Interface, Part::Files, Part::Reading, Part::Code];

    fn ix(self) -> usize {
        self as usize
    }

    /// The setting choosing its size.
    pub fn key(self) -> &'static str {
        match self {
            Part::Interface => crate::settings::TEXT_SIZE_KEY,
            Part::Files => crate::settings::FILES_TEXT_SIZE_KEY,
            Part::Reading => crate::settings::READING_TEXT_SIZE_KEY,
            Part::Code => crate::settings::CODE_TEXT_SIZE_KEY,
        }
    }

    /// Its body text at 100 %, in pixels, where its lengths are written.
    pub fn base(self) -> f32 {
        match self {
            Part::Interface | Part::Files | Part::Code => 14.,
            Part::Reading => 16.,
        }
    }

    /// How the user knows it: what a change of its size says.
    pub fn title(self) -> &'static str {
        match self {
            Part::Interface => "Text size",
            Part::Files => "Files text",
            Part::Reading => "Reading text",
            Part::Code => "Code text",
        }
    }

    /// The part a command names (`ui.text_size`'s `part`).
    pub fn named(name: &str) -> Option<Self> {
        Some(match name {
            "interface" => Part::Interface,
            "files" => Part::Files,
            "reading" => Part::Reading,
            "code" => Part::Code,
            _ => return None,
        })
    }
}

/// The step a setting says, `DEFAULT_STEP` when it says none of them.
pub fn step_of(value: Option<&str>) -> u32 {
    value
        .and_then(|value| value.trim().trim_end_matches('%').trim().parse().ok())
        .filter(|step| STEPS.contains(step))
        .unwrap_or(DEFAULT_STEP)
}

/// The step after `step` going `dir` (`in` larger, `out` smaller, else
/// back to 100 %), staying at the ends.
pub fn stepped(step: u32, dir: &str) -> u32 {
    let at = STEPS.iter().position(|s| *s == step);
    match (dir, at) {
        ("in", Some(at)) => STEPS[(at + 1).min(STEPS.len() - 1)],
        ("out", Some(at)) => STEPS[at.saturating_sub(1)],
        // A size not among the steps, or `reset`: back to 100 %.
        _ => DEFAULT_STEP,
    }
}

/// The preview's step after `percent` going larger or smaller: the next
/// of `PREVIEW_STEPS` past it, staying at the ends. A size between two
/// steps goes to the one beside it.
pub fn preview_stepped(percent: u32, larger: bool) -> u32 {
    if larger {
        PREVIEW_STEPS
            .iter()
            .copied()
            .find(|step| *step > percent)
            .unwrap_or(PREVIEW_STEPS[PREVIEW_STEPS.len() - 1])
    } else {
        PREVIEW_STEPS
            .iter()
            .rev()
            .copied()
            .find(|step| *step < percent)
            .unwrap_or(PREVIEW_STEPS[0])
    }
}

/// How a size is written, in the settings and the preview: `110%`.
pub fn step_label(step: u32) -> String {
    format!("{step}%")
}

/// The parts' sizes: each one's 100 %, in pixels, and the step its
/// setting chose. The files' 100 % is the interface's size.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Sizes {
    hundred: [f32; 4],
    step: [u32; 4],
}

impl Default for Sizes {
    /// Every part at its own 100 %.
    fn default() -> Self {
        Sizes {
            hundred: Part::ALL.map(Part::base),
            step: [DEFAULT_STEP; 4],
        }
    }
}

impl Sizes {
    /// The sizes the settings of `app` choose.
    pub fn resolve(app: &App) -> Self {
        Sizes {
            step: Part::ALL.map(|part| step_of(app.settings.get::<String>(part.key()).as_deref())),
            ..Sizes::default()
        }
    }

    /// The sizes with `part` at `step`: what a test or a sample draws.
    #[must_use]
    pub fn with_step(mut self, part: Part, step: u32) -> Self {
        self.step[part.ix()] = step;
        self
    }

    /// A part's body text.
    pub fn body(&self, part: Part) -> Pixels {
        px(self.of(part))
    }

    /// A part's body text, in pixels.
    pub fn of(&self, part: Part) -> f32 {
        self.hundred(part) * self.step(part) as f32 / 100.
    }

    /// A part's body text at 100 %, in pixels.
    pub fn hundred(&self, part: Part) -> f32 {
        match part {
            Part::Files => self.of(Part::Interface),
            _ => self.hundred[part.ix()],
        }
    }

    /// The step a part's setting chose.
    pub fn step(&self, part: Part) -> u32 {
        self.step[part.ix()]
    }

    /// The rem a part is drawn in: its body text is `text_sm` there.
    pub fn rem(&self, part: Part) -> Pixels {
        px(self.of(part) / BODY_REMS)
    }

    /// How much larger than at 100 % a part is drawn.
    pub fn scale(&self, part: Part) -> Scale {
        Scale(self.of(part) / part.base())
    }
}

/// How much larger than at 100 % something is drawn: lengths written at
/// 100 % are multiplied by it, in whole pixels.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Scale(pub f32);

impl Default for Scale {
    fn default() -> Self {
        Scale::ONE
    }
}

impl Scale {
    pub const ONE: Scale = Scale(1.);

    /// The scale of what is laid out now, from the rem in force (a part's,
    /// under `elements::rem`).
    pub fn of(window: &Window) -> Self {
        Scale(f32::from(window.rem_size()) / REM)
    }

    /// `at_100` pixels, at this scale, in whole pixels.
    pub fn px(self, at_100: f32) -> Pixels {
        px(self.f(at_100))
    }

    /// `px`, as a number.
    pub fn f(self, at_100: f32) -> f32 {
        (at_100 * self.0).round()
    }

    /// A length measured at this scale, back at 100 % (a column's width
    /// dragged, remembered whatever the size).
    pub fn at_100(self, measured: f32) -> f32 {
        if self.0 > 0. {
            measured / self.0
        } else {
            measured
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_step_is_read_whatever_is_written_around_it() {
        assert_eq!(step_of(Some("125")), 125);
        assert_eq!(step_of(Some(" 125 % ")), 125);
        assert_eq!(step_of(Some("120")), DEFAULT_STEP);
        assert_eq!(step_of(Some("large")), DEFAULT_STEP);
        assert_eq!(step_of(None), DEFAULT_STEP);
    }

    #[test]
    fn steps_stop_at_the_ends_and_go_back_to_100() {
        assert_eq!(stepped(100, "in"), 110);
        assert_eq!(stepped(100, "out"), 90);
        assert_eq!(stepped(200, "in"), 200);
        assert_eq!(stepped(80, "out"), 80);
        assert_eq!(stepped(150, "reset"), 100);
    }

    #[test]
    fn at_100_the_rem_is_gpui_s_and_lengths_are_as_written() {
        let sizes = Sizes::default();
        assert_eq!(sizes.rem(Part::Files), px(REM));
        assert_eq!(sizes.scale(Part::Files).px(26.), px(26.));
        assert_eq!(sizes.scale(Part::Reading), Scale::ONE);
    }

    #[test]
    fn the_preview_steps_past_the_size_it_is_at() {
        assert_eq!(preview_stepped(100, true), 110);
        assert_eq!(preview_stepped(100, false), 90);
        assert_eq!(preview_stepped(117, true), 125);
        assert_eq!(preview_stepped(117, false), 110);
        assert_eq!(preview_stepped(250, true), 250);
        assert_eq!(preview_stepped(60, false), 60);
    }

    #[test]
    fn the_files_100_is_the_interface_s_size() {
        let sizes = Sizes::default()
            .with_step(Part::Interface, 125)
            .with_step(Part::Files, 80);
        assert!((sizes.hundred(Part::Files) - 17.5).abs() < 1e-3);
        assert!((sizes.of(Part::Files) - 14.).abs() < 1e-3);
        assert!((sizes.of(Part::Reading) - 16.).abs() < 1e-3);
    }

    #[test]
    fn a_length_scales_in_whole_pixels() {
        let sizes = Sizes::default().with_step(Part::Files, 125);
        let scale = sizes.scale(Part::Files);
        assert_eq!(scale.px(26.), px(33.));
        assert_eq!(sizes.rem(Part::Files), px(20.));
        assert!((scale.at_100(225.) - 180.).abs() < 1e-3);
    }
}
