//! A row of texts and what acts on them (a setting's control, a plugin's
//! switch): side by side while the texts keep their widest word, and their
//! share of the row, beside the end at its narrowest; else the end on a
//! line of its own under them, against the right edge. A text narrowed
//! below its widest word cuts it ("themse / lves"), and a control pushed
//! past the row runs off the card: neither happens, at any width or text
//! size.
//!
//! The row is a wrapping flex line laid out by taffy. The texts start at
//! `share` of the row, and a flex item never starts narrower than its
//! least width, a text's widest word (vendored gpui 0061): they start at
//! the wider of the two. The end's narrowest and natural widths are
//! measured on it before the line is laid out, and the widths of its
//! versions where it holds a `Fit` (`adaptive::versions_laid_out`): a
//! choice's buttons, else its dropdown. In the line it starts at the width
//! it takes in what the texts' share leaves it, chosen once the row's
//! width is known (its flex basis fits its content within the row, vendored
//! gpui 0079): its first version that fits there, else anything from its
//! narrowest to its natural width (a field narrowing), never wider than
//! that room. Where nothing of it fits there, it starts at what it takes in
//! the whole row, and goes to the next line. Where it fits, the texts take
//! the rest: a dropdown in place of the buttons leaves them the buttons'
//! room. Alone on its line, the end grows to its natural width, and its
//! `Fit` draws the first version that fits the line.
//!
//! The room is the share's, not the texts' widest word, which the end
//! cannot know before the line is laid out: where that word is wider than
//! the share, a version chosen for the share that no longer fits beside
//! it goes to the next line, where a narrower one would have stayed.
//!
//! The end is drawn where its place in the line falls, laid out there in
//! `prepaint` as an adaptive row's version is: one line tall, its height
//! does not depend on its width. Its place is as tall as the line, the
//! end centered in it as a plain row centers it. The texts are an ordinary
//! child, as tall as they wrap.

use crate::sides::Sides;
use gpui::{
    AlignItems, AlignSelf, AnyElement, App, AvailableSpace, Bounds, Display, Div, Element,
    ElementId, FlexWrap, GlobalElementId, InspectorElementId, IntoElement, JustifyContent,
    LayoutId, Length, ParentElement as _, Pixels, Style, Styled as _, Window, div, fit_content, px,
    relative, size,
};

/// The gap between the texts and the end, and between their lines, at
/// 100 %: a card row's (`settings_view::card_row`, `gap_4`).
const GAP: f32 = 16.;
const LINE_GAP: f32 = 8.;
/// How much faster the texts grow than the end: beside them, the end keeps
/// the width it was given, and only grows on a line of its own.
const TEXTS_FIRST: f32 = 1e6;

/// The widths the end takes.
enum Widths {
    /// With each version of the `Fit` it holds, in the `Fit`'s order: a
    /// version's or none between two.
    Versions(Vec<Pixels>),
    /// Any from its narrowest to its natural width.
    Between(Pixels, Pixels),
}

impl Widths {
    /// What the end measures `narrowest` and `natural`, the widths of the
    /// versions of the `Fit`s it holds laid out with it: the versions of
    /// one, the rest of the end as it is (its natural width holds the
    /// `Fit` at its widest).
    fn of(narrowest: Pixels, natural: Pixels, laid_out: &[Vec<Pixels>]) -> Self {
        match laid_out {
            [versions] if versions.len() > 1 => {
                let widest = versions.iter().copied().fold(px(0.), Pixels::max);
                Self::Versions(
                    versions
                        .iter()
                        .map(|&version| (natural - widest + version).max(px(0.)))
                        .collect(),
                )
            }
            _ => Self::Between(narrowest, natural.max(narrowest)),
        }
    }

    /// The width the end takes in `room`, where it fits there: its first
    /// version no wider, else as wide as the room between its narrowest
    /// and its natural width.
    fn within(&self, room: Pixels) -> Option<Pixels> {
        match self {
            Self::Versions(widths) => widths.iter().copied().find(|&width| width <= room),
            &Self::Between(narrowest, natural) => (narrowest <= room).then(|| room.min(natural)),
        }
    }
}

pub struct Beside {
    texts: AnyElement,
    end: AnyElement,
    share: f32,
}

/// `texts` and `end` side by side, or `end` under `texts` where the texts
/// would be narrower than their widest word. The texts keep `share` of the
/// row, a fraction, whatever the end.
pub fn beside(texts: impl IntoElement, end: Div, share: f32) -> Beside {
    Beside {
        texts: texts.into_any_element(),
        // As wide as its place in the line: laid out on its own, a row is
        // as wide as what it holds whatever room it is given, so what
        // holds the end takes that place, and the end shrinks to it (a
        // `Fit` picking the version that fits there), against its right
        // edge by an automatic margin: one narrower than its narrowest
        // still starts where the texts do and runs out at the right.
        // As tall as its line, the end centered in it as a row centers what
        // it holds, its pixels rounded once.
        end: div()
            .size_full()
            .flex()
            .items_center()
            // Narrower than its narrowest where the line is, what it
            // holds giving way as it can (an empty revert slot).
            .child(
                div()
                    .margin_start_auto()
                    .min_w_0()
                    .flex()
                    .child(end.min_w_0()),
            )
            .into_any_element(),
        share,
    }
}

impl IntoElement for Beside {
    type Element = Self;
    fn into_element(self) -> Self {
        self
    }
}

/// The place left in the line for the end.
pub struct Laid {
    end: LayoutId,
}

impl Element for Beside {
    type RequestLayoutState = Laid;
    type PrepaintState = ();
    fn id(&self) -> Option<ElementId> {
        None
    }
    fn source_location(&self) -> Option<&'static std::panic::Location<'static>> {
        None
    }
    fn request_layout(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        window: &mut Window,
        cx: &mut App,
    ) -> (LayoutId, Laid) {
        let scale = crate::sizes::Scale::of(window);
        let tall = AvailableSpace::MaxContent;
        let ((narrowest, natural), laid_out) = super::adaptive::versions_laid_out(|| {
            let narrowest =
                self.end
                    .layout_as_root(size(AvailableSpace::MinContent, tall), window, cx);
            let natural = self
                .end
                .layout_as_root(AvailableSpace::max_size(), window, cx);
            (narrowest, natural)
        });
        let widths = Widths::of(narrowest.width, natural.width, &laid_out);
        let texts = self.texts.request_layout(window, cx);
        // Its least width left automatic: the texts' widest word.
        let held = window.request_layout(
            Style {
                display: Display::Flex,
                flex_basis: Length::Definite(relative(self.share)),
                flex_grow: TEXTS_FIRST,
                flex_shrink: 1.,
                ..Style::default()
            },
            [texts],
            cx,
        );
        let (gap, share) = (scale.px(GAP), self.share);
        let height = natural.height.max(narrowest.height);
        let end = window.request_measured_layout(
            Style {
                // Measured within the whole row, which it says it takes
                // beside the texts' share or, failing that, on its own.
                flex_basis: fit_content(relative(1.)),
                flex_grow: 1.,
                flex_shrink: 1.,
                // As tall as its line, never lower than the end. No least
                // width: in a line it shares it never shrinks (the line
                // holds both starts), and alone in one narrower than its
                // narrowest it takes the line, the end running out at the
                // right rather than the left.
                align_self: Some(AlignSelf::Stretch),
                min_size: size(
                    Length::Definite(px(0.).into()),
                    Length::Definite(height.into()),
                ),
                max_size: size(Length::Definite(natural.width.into()), Length::Auto),
                ..Style::default()
            },
            move |known, available, _, _| {
                let width = known.width.unwrap_or_else(|| match available.width {
                    AvailableSpace::MinContent => narrowest.width,
                    AvailableSpace::MaxContent => natural.width,
                    AvailableSpace::Definite(row) => widths
                        .within(row - gap - row * share)
                        .or_else(|| widths.within(row))
                        .unwrap_or(narrowest.width),
                });
                size(width, known.height.unwrap_or(height))
            },
        );
        let row = window.request_layout(
            Style {
                display: Display::Flex,
                flex_wrap: FlexWrap::Wrap,
                align_items: Some(AlignItems::Center),
                justify_content: Some(JustifyContent::FlexEnd),
                gap: size(scale.px(GAP).into(), scale.px(LINE_GAP).into()),
                flex_basis: Length::Definite(px(0.).into()),
                flex_grow: 1.,
                flex_shrink: 1.,
                min_size: size(Length::Definite(px(0.).into()), Length::Auto),
                ..Style::default()
            },
            [held, end],
            cx,
        );
        (row, Laid { end })
    }
    fn prepaint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        _: Bounds<Pixels>,
        laid: &mut Laid,
        window: &mut Window,
        cx: &mut App,
    ) {
        self.texts.prepaint(window, cx);
        let at = window.layout_bounds(laid.end);
        self.end.layout_as_root(
            size(
                AvailableSpace::Definite(at.size.width),
                AvailableSpace::Definite(at.size.height),
            ),
            window,
            cx,
        );
        self.end.prepaint_at(at.origin, window, cx);
    }
    fn paint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        _: Bounds<Pixels>,
        _: &mut Laid,
        (): &mut (),
        window: &mut Window,
        cx: &mut App,
    ) {
        self.texts.paint(window, cx);
        self.end.paint(window, cx);
    }
}

#[cfg(test)]
mod tests {
    use super::Widths;
    use gpui::px;

    /// A choice's buttons (120 px) else its dropdown (80 px), beside a
    /// revert slot: the end is as wide as either with the slot, and
    /// nothing between.
    #[test]
    fn an_end_holding_versions_takes_one_of_their_widths() {
        let widths = Widths::of(px(124.), px(164.), &[vec![px(120.), px(80.)]]);
        assert_eq!(widths.within(px(200.)), Some(px(164.)));
        assert_eq!(widths.within(px(164.)), Some(px(164.)));
        // Not the room between: what the buttons leave goes to the texts.
        assert_eq!(widths.within(px(150.)), Some(px(124.)));
        assert_eq!(widths.within(px(123.)), None);
    }

    /// Versions in the order their `Fit` gives them: the first that fits,
    /// as the `Fit` draws it, even where a later one is wider.
    #[test]
    fn the_first_version_that_fits_is_the_one_drawn() {
        let widths = Widths::of(px(60.), px(90.), &[vec![px(60.), px(90.)]]);
        assert_eq!(widths.within(px(100.)), Some(px(60.)));
        assert_eq!(widths.within(px(59.)), None);
    }

    /// An end without versions (a field) narrows with the room, from its
    /// natural width to its narrowest; as does one holding two `Fit`s,
    /// whose widths do not add up to one choice.
    #[test]
    fn an_end_without_versions_narrows_with_the_room() {
        let widths = Widths::of(px(100.), px(240.), &[]);
        assert_eq!(widths.within(px(300.)), Some(px(240.)));
        assert_eq!(widths.within(px(180.)), Some(px(180.)));
        assert_eq!(widths.within(px(99.)), None);
        let two = Widths::of(px(100.), px(240.), &[vec![px(90.), px(40.)], vec![px(50.)]]);
        assert_eq!(two.within(px(180.)), Some(px(180.)));
        // A `Fit` of one version is no choice.
        let one = Widths::of(px(100.), px(240.), &[vec![px(90.)]]);
        assert_eq!(one.within(px(180.)), Some(px(180.)));
    }
}
