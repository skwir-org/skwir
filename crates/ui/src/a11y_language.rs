//! The language a screen reader hears the window in (ADR-139): the one
//! its words are said in (the first of `locale::languages`: the language
//! chosen, else the first of the system's Skwir offers, else English), set
//! on the window's accessibility root. AccessKit hands it down to every
//! node, so a voice reads French words in French: Windows says it as each
//! node's UIA Culture, macOS as its text's language, AT-SPI as the
//! "language" attribute of each text (its `Locale` property stays empty:
//! `accesskit_unix` 0.22 answers none, so Orca does not hear it yet).
//!
//! A name, a document or a page written in another language still says
//! the window's: what it is written in is not known (a file's name), or
//! not yet said to the reader (a document's own `lang`).

use std::cell::RefCell;

use gpui::{SharedString, Window};

/// The window's language as a BCP 47 tag (`fr`, `en`), kept on the
/// window's thread until the languages move (`locale::revision`): a frame
/// asks it each time it is drawn.
pub fn of_window() -> SharedString {
    thread_local! {
        static KEPT: RefCell<Option<(u64, SharedString)>> = const { RefCell::new(None) };
    }
    let now = skwir_core::locale::revision();
    KEPT.with(|kept| {
        if let Some((at, tag)) = kept.borrow().as_ref()
            && *at == now
        {
            return tag.clone();
        }
        let tag: SharedString = skwir_core::locale::languages()
            .into_iter()
            .next()
            .unwrap_or_else(|| skwir_core::locale::ENGLISH.to_owned())
            .into();
        *kept.borrow_mut() = Some((now, tag.clone()));
        tag
    })
}

/// Says the window's language to assistive technology: once a frame, a
/// tag shared rather than made (`of_window`); gpui puts it on the tree's
/// root (patch 0073) only while a reader listens.
pub fn apply(window: &mut Window) {
    window.set_a11y_language(Some(of_window()));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_window_is_heard_in_the_language_its_words_are_said_in() {
        skwir_core::locale::set_chosen(None);
        skwir_core::locale::set_system(Vec::new());
        assert_eq!(of_window().as_ref(), "en");
        // A language chosen is the window's, kept until the languages move.
        skwir_core::locale::set_chosen(Some("fr".to_owned()));
        assert_eq!(of_window().as_ref(), "fr");
        skwir_core::locale::set_chosen(None);
        assert_eq!(of_window().as_ref(), "en");
    }
}
