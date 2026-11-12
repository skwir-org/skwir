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
//! measured on it before the line is laid out (a `Fit` in it says its
//! narrowest version and its widest, `adaptive`); it starts at its
//! narrowest and grows to its natural width before the texts take the
//! rest. Where both starts do not fit on one line, the end goes to the
//! next. Where they do, the row is laid out as a plain one is: the texts
//! take what the end leaves them, at least their share, the end shrinking
//! towards its narrowest for it.
//!
//! The end is drawn where its place in the line falls, laid out there in
//! `prepaint` as an adaptive row's version is: one line tall, its height
//! does not depend on its width. Its place is as tall as the line, the
//! end centered in it as a plain row centers it. The texts are an ordinary
//! child, as tall as they wrap.

use gpui::{
    AlignItems, AlignSelf, AnyElement, App, AvailableSpace, Bounds, Display, Div, Element,
    ElementId, FlexWrap, GlobalElementId, InspectorElementId, IntoElement, JustifyContent,
    LayoutId, Length, ParentElement as _, Pixels, Style, Styled as _, Window, div, px, relative,
    size,
};

/// The gap between the texts and the end, and between their lines, at
/// 100 %: a card row's (`settings_view::card_row`, `gap_4`).
const GAP: f32 = 16.;
const LINE_GAP: f32 = 8.;
/// How much faster the end grows than the texts: it reaches its natural
/// width before they take any of the room left.
const END_FIRST: f32 = 1e6;

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
            .child(div().ml_auto().min_w_0().flex().child(end.min_w_0()))
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
        let narrowest = self
            .end
            .layout_as_root(size(AvailableSpace::MinContent, tall), window, cx);
        let natural = self
            .end
            .layout_as_root(AvailableSpace::max_size(), window, cx);
        let texts = self.texts.request_layout(window, cx);
        // Its least width left automatic: the texts' widest word.
        let held = window.request_layout(
            Style {
                display: Display::Flex,
                flex_basis: Length::Definite(relative(self.share)),
                flex_grow: 1.,
                flex_shrink: 1.,
                ..Style::default()
            },
            [texts],
            cx,
        );
        let end = window.request_layout(
            Style {
                flex_basis: Length::Definite(narrowest.width.into()),
                flex_grow: END_FIRST,
                flex_shrink: 1.,
                // As tall as its line, never lower than the end. No least
                // width: in a line it shares it never shrinks (the line
                // holds both starts), and alone in one narrower than its
                // narrowest it takes the line, the end running out at the
                // right rather than the left.
                align_self: Some(AlignSelf::Stretch),
                min_size: size(
                    Length::Auto,
                    Length::Definite(natural.height.max(narrowest.height).into()),
                ),
                max_size: size(Length::Definite(natural.width.into()), Length::Auto),
                ..Style::default()
            },
            [],
            cx,
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
