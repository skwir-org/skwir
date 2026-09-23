//! How the window arranges its panes and the preview panel.
//!
//! Two choices, each with an automatic default: whether split panes sit side
//! by side or one above the other, and whether the preview is at the right
//! edge or at the bottom. Automatic follows the window's shape: a window
//! taller than it is wide (a portrait screen, half of a screen divided top
//! and bottom) stacks its panes and puts the preview under them. The
//! window's size is known before layout, so this is decided in
//! `Shell::render`, like whether the sidebar fits.
//!
//! A split made "the other way" (Shift+F3) overrides the panes' choice for
//! that split only; closing it forgets the override, so Automatic stays the
//! preference. Everything here is part of the window's shape (`session.rs`).

use gpui::{Context, Window};

use crate::shell::Shell;

/// How split panes are arranged.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Panes {
    #[default]
    Auto,
    /// Side by side.
    Row,
    /// One above the other.
    Column,
}

/// Where the preview panel goes.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Preview {
    #[default]
    Auto,
    Right,
    Bottom,
}

impl Panes {
    fn parse(s: &str) -> Option<Self> {
        match s {
            "auto" => Some(Self::Auto),
            "row" => Some(Self::Row),
            "column" => Some(Self::Column),
            _ => None,
        }
    }
}

impl Preview {
    fn parse(s: &str) -> Option<Self> {
        match s {
            "auto" => Some(Self::Auto),
            "right" => Some(Self::Right),
            "bottom" => Some(Self::Bottom),
            _ => None,
        }
    }
}

/// The user's choices, and what they resolved to for the current window.
#[derive(Clone, Debug, Default)]
pub struct Layout {
    pub panes: Panes,
    pub preview: Preview,
    /// The current split's own arrangement, when it was made the other way.
    pub split: Option<Panes>,
    /// Whether the window counts as tall; kept between the two thresholds
    /// so a window resized through square does not flip back and forth.
    tall: bool,
}

/// How much taller (or wider) than square a window must be to change what
/// Automatic chooses.
const HYSTERESIS: f32 = 1.1;

impl Layout {
    /// Follow the window's shape; call once per frame, before layout.
    pub fn observe(&mut self, width: f32, height: f32) {
        if height >= width * HYSTERESIS {
            self.tall = true;
        } else if width >= height * HYSTERESIS {
            self.tall = false;
        }
    }

    /// What the panes' preference alone gives in this window.
    fn preferred_stacked(&self) -> bool {
        match self.panes {
            Panes::Auto => self.tall,
            Panes::Row => false,
            Panes::Column => true,
        }
    }

    /// Whether split panes are one above the other.
    pub fn stacked(&self) -> bool {
        match self.split {
            Some(Panes::Row) => false,
            Some(Panes::Column) => true,
            _ => self.preferred_stacked(),
        }
    }

    /// Whether the preview panel is at the bottom.
    pub fn bottom(&self) -> bool {
        match self.preview {
            Preview::Auto => self.tall,
            Preview::Right => false,
            Preview::Bottom => true,
        }
    }

    /// The arrangement `axis` asks a split for: `row`, `column`, or `other`,
    /// the opposite of what the preference gives.
    fn split_for(&self, axis: &str) -> Option<Panes> {
        match axis {
            "other" if self.preferred_stacked() => Some(Panes::Row),
            "other" => Some(Panes::Column),
            axis => Panes::parse(axis).filter(|p| *p != Panes::Auto),
        }
    }

    /// What Automatic currently means, for the menu.
    pub fn tall(&self) -> bool {
        self.tall
    }

    /// The Split toggle's icon and tooltip: the split it makes, or shows.
    pub fn split_button(&self) -> (&'static str, &'static str) {
        if self.stacked() {
            ("split-rows", "Split panes one above the other")
        } else {
            ("split", "Split panes side by side")
        }
    }

    /// The Preview toggle's icon: where the panel opens, or is.
    pub fn preview_icon(&self) -> &'static str {
        if self.bottom() {
            "preview-bottom"
        } else {
            "preview"
        }
    }
}

impl Shell {
    /// `ui.split { axis? }`. Without an axis, split or close the split as
    /// the preferences say. With one (`row`, `column`, `other`), split that
    /// way; already split that way, close it; split the other way, turn it.
    pub fn split_command(
        &mut self,
        args: &serde_json::Value,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let axis = args.get("axis").and_then(serde_json::Value::as_str);
        let Some(wanted) = axis.and_then(|a| self.layout.split_for(a)) else {
            self.toggle_split(window, cx);
            return;
        };
        let stacked = wanted == Panes::Column;
        if !self.is_split() {
            self.layout.split = (stacked != self.layout.preferred_stacked()).then_some(wanted);
            self.toggle_split(window, cx);
        } else if self.layout.stacked() == stacked {
            self.toggle_split(window, cx);
        } else {
            self.layout.split = (stacked != self.layout.preferred_stacked()).then_some(wanted);
            self.remember_session();
            cx.notify();
        }
    }

    /// `ui.layout { panes?, preview? }`: choose the arrangement. Choosing
    /// the panes' arrangement also turns a split made the other way.
    pub fn layout_command(&mut self, args: &serde_json::Value, cx: &mut Context<Self>) {
        let arg = |key| args.get(key).and_then(serde_json::Value::as_str);
        let panes = arg("panes").map(|v| (v, Panes::parse(v)));
        let preview = arg("preview").map(|v| (v, Preview::parse(v)));
        if let Some((v, None)) = panes {
            self.set_status(format!("“{v}” is not a way to arrange panes"), true, cx);
            return;
        }
        if let Some((v, None)) = preview {
            self.set_status(format!("“{v}” is not a place for the preview"), true, cx);
            return;
        }
        if let Some((_, Some(panes))) = panes {
            self.layout.panes = panes;
            self.layout.split = None;
        }
        if let Some((_, Some(preview))) = preview {
            self.layout.preview = preview;
        }
        self.remember_session();
        cx.notify();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn automatic_follows_the_window_shape_without_flickering_near_square() {
        let mut layout = Layout::default();
        layout.observe(1600., 900.);
        assert!(!layout.stacked() && !layout.bottom());
        layout.observe(900., 1600.);
        assert!(layout.stacked() && layout.bottom());
        // Nearly square: whatever it was stays.
        layout.observe(1000., 1040.);
        assert!(layout.stacked());
        layout.observe(1040., 1000.);
        assert!(layout.stacked());
        layout.observe(1200., 1000.);
        assert!(!layout.stacked());
    }

    #[test]
    fn a_split_the_other_way_overrides_only_that_split() {
        let mut layout = Layout::default();
        layout.observe(1600., 900.);
        assert_eq!(layout.split_for("other"), Some(Panes::Column));
        layout.split = Some(Panes::Column);
        assert!(layout.stacked());
        assert_eq!(layout.panes, Panes::Auto, "the preference is untouched");
        layout.split = None;
        assert!(!layout.stacked());
        assert_eq!(layout.split_for("auto"), None);
        assert_eq!(layout.split_for("row"), Some(Panes::Row));
    }

    #[test]
    fn explicit_choices_ignore_the_shape() {
        let mut layout = Layout {
            panes: Panes::Row,
            preview: Preview::Bottom,
            ..Layout::default()
        };
        layout.observe(900., 1600.);
        assert!(!layout.stacked());
        assert!(layout.bottom());
    }

    #[test]
    fn choices_serialize_as_words() {
        assert_eq!(
            serde_json::to_value(Panes::Column).unwrap(),
            serde_json::json!("column")
        );
        assert_eq!(
            serde_json::from_value::<Preview>(serde_json::json!("bottom")).unwrap(),
            Preview::Bottom
        );
    }
}
