//! Virtual source lines. Fold coordinates stay in the provider's logical lines.
//!
//! The code view draws code colored by `skwir_platform::highlight` (bat's
//! grammars and themes, ADR-060), in skwir's look or the one the Reading
//! page keeps it in. Code in a look that is not skwir's is drawn with that
//! look's tokens (`Theme::in_look`).
use crate::{inspector_text::Selection, shell::Shell, theme::Theme};
use gpui::{AnyElement, Hsla, WeakEntity, div, prelude::*, px};
use skwir_core::{
    inspection::{Block, Inspection},
    workspace::PaneId,
};
use std::collections::BTreeSet;

pub use skwir_platform::highlight::{
    CodeTheme, DEFAULT, Highlighter, Palette, Resume, UserTheme, choices, user_themes,
};

/// A color kept as a run's is (`Run::color`), to draw with.
pub fn color([r, g, b]: [u8; 3]) -> Hsla {
    gpui::rgb((u32::from(r) << 16) | (u32::from(g) << 8) | u32::from(b)).into()
}

pub fn visible(data: &Inspection, collapsed: &BTreeSet<usize>) -> Vec<usize> {
    let mut hidden_until = 0;
    let mut folds = data.folds.iter().peekable();
    (0..data.blocks.len())
        .filter(|&line| {
            while folds.peek().is_some_and(|fold| fold.start < line) {
                folds.next();
            }
            if line < hidden_until {
                return false;
            }
            if let Some(fold) = folds.peek().filter(|fold| fold.start == line)
                && collapsed.contains(&line)
            {
                hidden_until = fold.end.min(data.blocks.len());
            }
            true
        })
        .collect()
}

/// A row of the code view: a line of code, or a row of a text read where
/// it is shown (ADR-051), its number in the gutter.
pub struct CodeRow<'a> {
    pub block: &'a Block,
    /// Its index among the rows: what selection, links and ids count.
    pub index: usize,
    /// The line number in the gutter (from 1), on a line's first row.
    pub number: Option<u64>,
    /// How many digits the gutter holds: the most any row of it needs.
    pub digits: usize,
    /// The fold it opens, and whether it is closed.
    pub fold: Option<(&'a std::ops::Range<usize>, bool)>,
    /// How far its text is scrolled sideways, when lines do not wrap: the
    /// gutter stays.
    pub shift: f32,
    pub selected: bool,
    pub selection: Option<Selection>,
    pub font_size: f32,
    pub mono: &'a gpui::SharedString,
    pub theme: &'a Theme,
    /// The inspector's style for this frame, built once, not per row.
    pub style: &'a crate::markdown::MarkdownStyle,
    pub pane: PaneId,
    pub entity: WeakEntity<Shell>,
}

/// The digits a gutter holds for `rows` lines: five at least, so it does
/// not widen as a short text's count grows.
pub fn digits(rows: u64) -> usize {
    rows.max(1).ilog10() as usize + 1
}

impl CodeRow<'_> {
    pub fn render(self) -> AnyElement {
        let Self {
            block,
            index,
            number,
            digits,
            fold,
            shift,
            selected,
            selection,
            font_size,
            mono,
            theme,
            style,
            pane,
            entity,
        } = self;
        let folded = fold.is_some_and(|(_, folded)| folded);
        // A digit of the monospaced font is about 0.6 of its size.
        let numbers = (digits.max(5) as f32 * (font_size - 2.) * 0.62).ceil();
        let mut gutter = div()
            .id(("code-fold", index))
            .flex_none()
            .flex()
            .items_center()
            .w(px(numbers + 30.))
            .pl_1()
            .pr_2()
            .gap_1()
            .text_color(theme.text_muted)
            .child(
                div()
                    .w(px(numbers))
                    .text_right()
                    .children(number.map(|number| number.to_string())),
            )
            .child(div().w(px(14.)).child(match fold {
                Some((_, true)) => "▸",
                Some((_, false)) => "▾",
                None => "",
            }));
        if fold.is_some() {
            let weak = entity.clone();
            gutter = gutter
                .cursor_pointer()
                .hover(|d| d.bg(theme.hover))
                .on_click(move |_, window, cx| {
                    let _ = weak.update(cx, |s, cx| {
                        s.with_pane(pane, |s| {
                            s.run(
                                "ui.inspect",
                                serde_json::json!({"action":"collapse", "index":index}),
                                window,
                                cx,
                            );
                        });
                    });
                });
        }
        div()
            .id(("code-line", index))
            .w_full()
            .flex()
            .items_start()
            .font_family(mono.clone())
            .text_size(px(font_size - 2.))
            .line_height(px(font_size + 4.))
            .when(selected, |d| d.bg(theme.highlight))
            .child(gutter)
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .pr_2()
                    .when(shift > 0., gpui::Styled::overflow_hidden)
                    // Moved, not laid out shorter: the row measures as
                    // wide however far it is scrolled, so the rows know
                    // how far they can go (`elements::rows`).
                    .child(
                        div()
                            .relative()
                            .left(px(-shift))
                            .child(crate::inspector_text::render(
                                block, index, selection, style, pane, entity,
                            )),
                    ),
            )
            .when_some(fold.filter(|_| folded), |d, (range, _)| {
                d.child(
                    div()
                        .flex_none()
                        .px_2()
                        .rounded_sm()
                        .bg(theme.code_bg)
                        .text_color(theme.text_muted)
                        .child(format!("… {} lines", range.end - range.start - 1)),
                )
            })
            .into_any_element()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use skwir_core::inspection::{Block, InspectionKind};

    #[test]
    fn nested_folds_retain_siblings_and_remember_child_state() {
        let data = Inspection {
            kind: InspectionKind::Code,
            blocks: vec![Block::default(); 10],
            folds: vec![0..7, 2..5, 7..9],
            ..Inspection::default()
        };
        assert_eq!(
            visible(&data, &BTreeSet::from([2])),
            [0, 1, 2, 5, 6, 7, 8, 9]
        );
        assert_eq!(visible(&data, &BTreeSet::from([0, 2])), [0, 7, 8, 9]);
        assert_eq!(visible(&data, &BTreeSet::from([0, 2, 7])), [0, 7, 9]);
        assert_eq!(
            visible(&data, &BTreeSet::new()),
            (0..10).collect::<Vec<_>>()
        );
    }

    /// Code is as light or dark as skwir unless the settings keep it in a
    /// look, and wears the theme chosen for the look it is in.
    #[test]
    fn code_is_in_skwir_s_look_unless_the_settings_keep_it_in_one() {
        let tmp = tempfile::tempdir().unwrap();
        let app = skwir_core::App::new(skwir_core::app::AppConfig::ephemeral(tmp.path())).unwrap();
        crate::settings::declare(&app);
        app.settings
            .set(crate::settings::CODE_LIGHT_KEY, "GitHub".into());
        app.settings
            .set(crate::settings::CODE_DARK_KEY, "Dracula".into());
        let chosen = |skwir_dark| {
            let code = CodeTheme::chosen(&app, skwir_dark);
            (code.dark, code.name.to_string())
        };
        let (light, dark) = ((false, "GitHub".to_owned()), (true, "Dracula".to_owned()));
        assert_eq!((chosen(false), chosen(true)), (light.clone(), dark.clone()));
        app.settings
            .set(crate::settings::CODE_APPEARANCE_KEY, "light".into());
        assert_eq!(
            (chosen(false), chosen(true)),
            (light.clone(), light.clone())
        );
        app.settings
            .set(crate::settings::CODE_APPEARANCE_KEY, "dark".into());
        assert_eq!((chosen(false), chosen(true)), (dark.clone(), dark));
        app.settings
            .set(crate::settings::CODE_APPEARANCE_KEY, "auto".into());
        assert_eq!(chosen(false), light);
    }

    /// Code kept in the other look is drawn with that look's tokens, its
    /// sizes and measure those of the document around it.
    #[test]
    fn code_in_the_other_look_is_drawn_with_its_tokens() {
        let dark = Theme::dark();
        assert!(matches!(dark.in_look(true), std::borrow::Cow::Borrowed(_)));
        let light = dark.in_look(false);
        assert!(!light.is_dark);
        assert_eq!(
            (light.bg, light.text),
            (Theme::light().bg, Theme::light().text)
        );
        assert_eq!(light.code, dark.code, "what code is drawn in stays");
        let document = crate::markdown::MarkdownStyle::new(&dark, 16., true, Some(42.));
        let code = document.drawn_in(&light);
        assert_eq!(
            (code.text, code.selection_bg, code.code_bg),
            (light.text, light.selection_bg, light.code_bg)
        );
        assert_eq!(
            (code.code_size, code.measure, code.line_height),
            (document.code_size, document.measure, document.line_height)
        );
        assert_eq!(document.code().text, dark.text, "no look of its own");
        let mut document = document;
        document.code_look = Some(std::rc::Rc::new(code));
        assert_eq!(document.code().text, light.text);
    }
}
