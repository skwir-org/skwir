//! A text cut to fit says the whole of it in a tooltip, only while cut.
//!
//! The box a text sits in says how it is cut (`truncate`,
//! `text_ellipsis_middle`, a `line_clamp`); whether it was is known only once
//! it has its width. The text element keeps what it drew, so the tooltip
//! compares that with the whole when the pointer has rested on it, never in
//! `render`. While the text fits, the tooltip declines
//! (`optional_tooltip`, vendored patch 0011), and a tooltip of an element the
//! text lies in shows as before. An element with a tooltip of its own passes
//! it as `also`: the whole text comes first, then that, on its own line.
use gpui::{AnyView, App, Div, SharedString, Stateful, StyledText, TextLayout, Window, prelude::*};

/// What tells whether a text was cut, and what it said whole.
#[derive(Clone)]
pub struct Clip {
    whole: SharedString,
    layout: TextLayout,
}

/// The text to draw where it may be cut, and its `Clip`.
pub fn text(text: impl Into<SharedString>) -> (StyledText, Clip) {
    let whole = text.into();
    let styled = StyledText::new(whole.clone());
    let layout = styled.layout().clone();
    (styled, Clip { whole, layout })
}

impl Clip {
    /// Whether the text as last drawn is not all of it. Asked only once it
    /// was drawn: its tooltip is registered as it paints.
    fn cut(&self) -> bool {
        self.layout.text() != self.whole.as_ref()
    }

    /// What a tooltip says: the whole text while cut, with `also` after
    /// it; `also` alone while it fits, or when it says the text already.
    pub fn says(&self, also: Option<&SharedString>) -> Option<SharedString> {
        match also {
            Some(also) if !self.cut() || also.contains(self.whole.as_ref()) => Some(also.clone()),
            Some(also) => Some(format!("{}\n{also}", self.whole).into()),
            None => self.cut().then(|| self.whole.clone()),
        }
    }

    /// A builder for `optional_tooltip` saying what `says` says.
    pub fn tooltip(
        self,
        also: Option<SharedString>,
    ) -> impl Fn(&mut Window, &mut App) -> Option<AnyView> + 'static {
        move |_, cx| {
            let tip = self.says(also.as_ref())?;
            Some(crate::toolbar::tooltip(tip, cx))
        }
    }
}

/// `el` showing `text`, cut as `el`'s style says, and saying all of it in a
/// tooltip while cut.
pub fn clipped(el: Stateful<Div>, text: impl Into<SharedString>) -> Stateful<Div> {
    clipped_with(el, text, None)
}

/// `clipped` inside an element with a tooltip of its own, `also`: pointed
/// at while cut, the text says itself, then that.
pub fn clipped_with(
    el: Stateful<Div>,
    text: impl Into<SharedString>,
    also: Option<SharedString>,
) -> Stateful<Div> {
    let (text, clip) = self::text(text);
    el.optional_tooltip(clip.tooltip(also)).child(text)
}
