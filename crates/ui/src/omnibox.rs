//! The address field as the one place to type: a path, a search, a place
//! to go to, or a command.
//!
//! Ctrl+F (`ui.search`) starts a search: the field shows where it will
//! look as a pill before the text (the folder shown, or the root of the
//! search shown, whose text is then edited). Ctrl+L (`ui.focus_address`)
//! edits the path. Either way, once the text is edited, a dropdown under
//! the field says what Enter will do and offers the rest:
//!
//! - text that names a location (a path, `~`, `scheme:`) goes there;
//! - words search where the pill says (Enter) or the whole home folder
//!   (Alt+Enter), look for files containing them, or go to a place Jump
//!   knows (`jump.candidates`, asked on a worker);
//! - `>` hands the rest to the command palette.
//!
//! The UI knows no query syntax: a search is `query.search` with the text
//! as `expr` and the pill's folder as `root`, and files containing the text
//! are the plugin's own refinement (`query.refine`, facet `content`). The
//! keyboard never leaves the field: Up and Down move through the dropdown
//! (bound in the address bar's context), a click on a row runs it without
//! taking the focus.

use std::path::{Path, PathBuf};

use bibli_core::command::{Invocation, Outcome};
use bibli_core::os_path::PathArg;
use gpui::{AnyElement, Context, SharedString, Window, div, prelude::*, px};

use crate::elements::popover::PopoverAnchor;
use crate::shell::Shell;
use crate::theme::Theme;

/// The most places Jump adds under the searches.
const PLACES: usize = 5;

/// A pane's field while it is being typed in.
#[derive(Default)]
pub struct Omni {
    /// Where a search looks, drawn as a pill before the text; `None` while
    /// a path is edited (the folder shown is then where words search).
    pub scope: Option<PathBuf>,
    /// The text changed since the field took the keyboard: until then it
    /// is what the field showed, and Enter goes there as before.
    edited: bool,
    highlighted: usize,
    /// Jump's places for `asked`.
    places: Vec<crate::jump::Candidate>,
    asked: Option<String>,
}

/// What a row of the dropdown does.
#[derive(Clone, Debug, PartialEq)]
enum Suggestion {
    /// Go to the location the text names.
    Go,
    /// Search for the text where the pill says, or everywhere at home.
    Search { everywhere: bool },
    /// Files whose contents hold the text, where the pill says.
    Contains,
    /// A place Jump knows.
    Place(PathBuf),
}

/// Whether `text` is a path: absolute, from home, or relative.
fn is_path(text: &str) -> bool {
    let text = text.trim_start();
    text.starts_with('/') || text.starts_with('~') || text.starts_with("./")
}

/// The scheme `text` starts with (`scheme:rest`), whether or not a
/// provider has it.
fn scheme_of(text: &str) -> Option<&str> {
    let (scheme, _) = text.trim_start().split_once(':')?;
    (!scheme.is_empty()
        && scheme
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-'))
    .then_some(scheme)
}

fn home() -> Option<PathBuf> {
    std::env::var_os("HOME").map(PathBuf::from)
}

/// A folder as the pill and the rows say it.
fn folder_name(path: &Path) -> String {
    if home().as_deref() == Some(path) {
        return "Home".to_owned();
    }
    path.file_name().map_or_else(
        || path.display().to_string(),
        |name| name.to_string_lossy().into_owned(),
    )
}

impl Shell {
    /// Whether `text` names a location rather than words to look for: a
    /// path, or a scheme a provider opens (`query:`, `shelf:`; `kind:` is
    /// a word of the query language).
    fn names_location(&self, text: &str) -> bool {
        is_path(text) || scheme_of(text).is_some_and(|s| self.app.provider(s).is_some())
    }

    /// Give the active pane's field the keyboard: `search` (Ctrl+F) with
    /// where it looks as a pill and `text` to start from, else the path
    /// (Ctrl+L). A pane showing a search edits that search either way.
    pub fn omnibox_begin(
        &mut self,
        search: bool,
        text: Option<String>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let pane = self.pane;
        let chips = &self.ui().chips;
        let (scope, initial, select) = if !chips.chips.is_empty() {
            let given = text.is_some();
            (
                Some(chips.root.clone()),
                text.unwrap_or_else(|| chips.text.clone()),
                !given,
            )
        } else if search {
            (
                self.app.pane_dir(pane).ok(),
                text.unwrap_or_default(),
                false,
            )
        } else {
            (None, self.ui().address_shown.clone(), true)
        };
        let placeholder = match &scope {
            Some(root) => format!("Search in {}", folder_name(root)),
            None => "A path, or words to search".to_owned(),
        };
        self.ui_mut().omni = Some(Omni {
            scope,
            ..Omni::default()
        });
        let address = self.ui().address.clone();
        address.update(cx, |input, cx| {
            input.set_placeholder(placeholder, cx);
            input.set_text(initial, cx);
            if select {
                input.select_all_now(cx);
            }
            input.focus(window);
        });
        cx.notify();
    }

    /// The field's text changed: the dropdown follows, Jump is asked for
    /// places, and `>` goes to the palette.
    pub fn omnibox_changed(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let text = self.ui().address.read(cx).text().to_owned();
        if let Some(rest) = text.strip_prefix('>') {
            let rest = rest.trim_start().to_owned();
            self.omnibox_cancel(window, cx);
            self.open_palette(window, cx);
            if let Some(palette) = &self.palette {
                palette
                    .input
                    .update(cx, |input, cx| input.set_text(rest, cx));
            }
            return;
        }
        let words = text.trim().to_owned();
        let no_places = words.is_empty()
            || self.names_location(&words)
            || self.app.commands.get("jump.candidates").is_none();
        let omni = self.ui_mut().omni.get_or_insert_with(Omni::default);
        omni.edited = true;
        omni.highlighted = 0;
        if no_places {
            omni.places.clear();
            omni.asked = None;
        } else if omni.asked.as_deref() != Some(words.as_str()) {
            omni.asked = Some(words.clone());
            let pane = self.pane;
            let app = std::sync::Arc::clone(&self.app);
            let args = serde_json::json!({ "query": words, "limit": PLACES });
            self.off_thread(
                "omnibox-places",
                move || app.dispatch(&Invocation::with_args("jump.candidates", &args)),
                move |shell, result, _, cx| {
                    let Some(omni) = shell
                        .pane_ui_mut(pane)
                        .and_then(|ui| ui.omni.as_mut())
                        .filter(|o| o.asked.as_deref() == Some(words.as_str()))
                    else {
                        return;
                    };
                    omni.places = match result {
                        Ok(Outcome::Value(value)) => {
                            serde_json::from_value(value).unwrap_or_default()
                        }
                        _ => Vec::new(),
                    };
                    omni.places.truncate(PLACES);
                    cx.notify();
                },
                window,
                cx,
            );
        }
        cx.notify();
    }

    /// Up or Down in the dropdown (`ui.omnibox_move { delta }`).
    pub fn omnibox_move(&mut self, delta: isize, cx: &mut Context<Self>) {
        let count = self.suggestions(cx).len();
        if let Some(omni) = self.ui_mut().omni.as_mut()
            && count > 0
        {
            let at = omni.highlighted.cast_signed() + delta;
            omni.highlighted = at.rem_euclid(count.cast_signed()).cast_unsigned();
            cx.notify();
        }
    }

    /// Enter (the row highlighted) or Alt+Enter (`everywhere`: a search of
    /// the home folder). Text not edited goes where it says, as before.
    pub fn omnibox_submit(
        &mut self,
        everywhere: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let text = self.ui().address.read(cx).text().trim().to_owned();
        let suggestions = self.suggestions(cx);
        let chosen = if everywhere {
            suggestions
                .iter()
                .find(|s| matches!(s, Suggestion::Search { .. }))
                .map(|_| Suggestion::Search { everywhere: true })
        } else {
            let at = self.ui().omni.as_ref().map_or(0, |o| o.highlighted);
            suggestions.get(at).cloned()
        };
        let scope = self.omni_root();
        let pane = self.pane;
        self.ui_mut().omni = None;
        window.focus(&self.ui().focus);
        match chosen {
            _ if text.is_empty() => {
                let shown = self.ui().address_shown.clone();
                let address = self.ui().address.clone();
                address.update(cx, |input, cx| input.set_text(shown, cx));
            }
            None | Some(Suggestion::Go) => {
                self.run(
                    "navigate",
                    serde_json::json!({ "location": text, "pane": pane }),
                    window,
                    cx,
                );
            }
            Some(Suggestion::Search { everywhere }) => {
                let root = if everywhere { home() } else { scope };
                self.ui_mut().last_search.clone_from(&text);
                self.run(
                    "query.search",
                    serde_json::json!({
                        "expr": text,
                        "root": root.as_deref().map(PathArg::from),
                        "pane": pane,
                    }),
                    window,
                    cx,
                );
            }
            Some(Suggestion::Contains) => {
                self.run(
                    "query.refine",
                    serde_json::json!({ "facet": "content", "value": text, "pane": pane }),
                    window,
                    cx,
                );
            }
            Some(Suggestion::Place(path)) => {
                self.reveal_path(pane, &path, window, cx);
            }
        }
        cx.notify();
    }

    /// Escape: the field shows where the pane is again.
    pub fn omnibox_cancel(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.ui_mut().omni = None;
        let shown = self.ui().address_shown.clone();
        let address = self.ui().address.clone();
        address.update(cx, |input, cx| input.set_text(shown, cx));
        window.focus(&self.ui().focus);
        cx.notify();
    }

    /// Where the active pane's words search: the pill's folder, else the
    /// folder shown.
    fn omni_root(&self) -> Option<PathBuf> {
        self.ui()
            .omni
            .as_ref()
            .and_then(|o| o.scope.clone())
            .or_else(|| self.app.pane_dir(self.pane).ok())
    }

    /// The dropdown's rows for the active pane's text, first the one Enter
    /// runs. Nothing until the text is edited.
    fn suggestions(&self, cx: &gpui::App) -> Vec<Suggestion> {
        let Some(omni) = self.ui().omni.as_ref().filter(|o| o.edited) else {
            return Vec::new();
        };
        let text = self.ui().address.read(cx).text().trim().to_owned();
        if text.is_empty() {
            return Vec::new();
        }
        if self.names_location(&text) {
            return vec![Suggestion::Go];
        }
        let mut rows = Vec::new();
        if self.app.commands.get("query.search").is_some() {
            rows.push(Suggestion::Search { everywhere: false });
            if home().is_some() && home() != self.omni_root() {
                rows.push(Suggestion::Search { everywhere: true });
            }
            // Contents narrow the folder shown; a search shown is edited as
            // a whole instead.
            if self.ui().chips.chips.is_empty() && self.app.pane_dir(self.pane).is_ok() {
                rows.push(Suggestion::Contains);
            }
        }
        rows.extend(
            omni.places
                .iter()
                .map(|c| Suggestion::Place(c.path.clone())),
        );
        if rows.is_empty() {
            rows.push(Suggestion::Go);
        }
        rows
    }
}

/// Where the search typed will look, as a pill before the text: only while
/// the active pane's field is being typed in, in search.
pub fn scope_pill(shell: &Shell, cx: &mut Context<Shell>) -> Option<AnyElement> {
    let scope = shell.ui().omni.as_ref()?.scope.as_ref()?;
    let theme = cx.global::<Theme>().clone();
    Some(
        div()
            .flex_none()
            .ml(px(3.))
            .h(px(22.))
            .px(px(8.))
            .flex()
            .items_center()
            .gap(px(6.))
            .rounded_full()
            .border_1()
            .border_color(theme.border)
            .text_xs()
            .text_color(theme.text_muted)
            .child(
                gpui::svg()
                    .path("toolbar/folder.svg")
                    .size(px(14.))
                    .flex_none()
                    .text_color(theme.text_muted),
            )
            .child(format!("In {}", folder_name(scope)))
            .into_any_element(),
    )
}

/// The dropdown under the field of the pane being rendered, while it is
/// typed in and its text says something; `anchor` is the field's.
pub fn dropdown(
    shell: &Shell,
    anchor: &PopoverAnchor,
    cx: &mut Context<Shell>,
) -> Option<AnyElement> {
    let suggestions = shell.suggestions(cx);
    if suggestions.is_empty() {
        return None;
    }
    let omni = shell.ui().omni.as_ref()?;
    let theme = cx.global::<Theme>().clone();
    let text = shell.ui().address.read(cx).text().trim().to_owned();
    let root = shell.omni_root();
    let pane = shell.pane;
    let mut panel = div()
        .min_w(px(360.))
        .max_w(px(560.))
        .p_1()
        .flex()
        .flex_col()
        .rounded_lg()
        .border_1()
        .border_color(theme.border)
        .bg(theme.bg)
        .text_color(theme.text)
        .shadow_md()
        .occlude();
    let mut places_titled = false;
    for (i, suggestion) in suggestions.iter().enumerate() {
        let (icon, title, detail, keys): (&str, String, Option<String>, Option<&str>) =
            match suggestion {
                Suggestion::Go => ("open", format!("Go to {text}"), None, None),
                Suggestion::Search { everywhere: false } => (
                    "search",
                    format!(
                        "“{text}” in {} and its folders",
                        root.as_deref()
                            .map_or_else(|| "this folder".to_owned(), folder_name)
                    ),
                    None,
                    None,
                ),
                Suggestion::Search { everywhere: true } => (
                    "search",
                    format!("“{text}” everywhere in Home"),
                    None,
                    Some("Alt+Enter"),
                ),
                Suggestion::Contains => {
                    ("contents", format!("Files containing “{text}”"), None, None)
                }
                Suggestion::Place(path) => (
                    if path.is_dir() { "folder" } else { "open" },
                    path.file_name().map_or_else(
                        || path.display().to_string(),
                        |n| n.to_string_lossy().into_owned(),
                    ),
                    path.parent().map(|p| p.display().to_string()),
                    None,
                ),
            };
        if matches!(suggestion, Suggestion::Place(_)) && !places_titled {
            places_titled = true;
            panel = panel.child(
                div()
                    .px_2()
                    .pt_2()
                    .pb_1()
                    .text_xs()
                    .text_color(theme.text_muted)
                    .child("Go to"),
            );
        }
        let highlighted = i == omni.highlighted;
        let keys = if highlighted { Some("Enter") } else { keys };
        let hover = theme.hover;
        let muted = if highlighted {
            theme.selection_text
        } else {
            theme.text_muted
        };
        panel = panel.child(
            div()
                .id(("omnibox", i))
                .h(px(30.))
                .px_2()
                .flex()
                .items_center()
                .gap(px(10.))
                .rounded_md()
                .cursor_pointer()
                .when(highlighted, |d| {
                    d.bg(theme.selection_bg).text_color(theme.selection_text)
                })
                .when(!highlighted, |d| d.hover(move |s| s.bg(hover)))
                // The field keeps the keyboard: the row acts on the press,
                // before anything under it could take the focus.
                .on_mouse_down(
                    gpui::MouseButton::Left,
                    cx.listener(move |this, _, window, cx| {
                        cx.stop_propagation();
                        window.prevent_default();
                        this.activate_pane(pane, cx);
                        if let Some(omni) = this.ui_mut().omni.as_mut() {
                            omni.highlighted = i;
                        }
                        this.omnibox_submit(false, window, cx);
                    }),
                )
                .child(
                    gpui::svg()
                        .path(format!("toolbar/{icon}.svg"))
                        .size(px(15.))
                        .flex_none()
                        .text_color(muted),
                )
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .overflow_hidden()
                        .whitespace_nowrap()
                        .text_ellipsis_middle()
                        .child(SharedString::from(title)),
                )
                .children(detail.map(|detail| {
                    div()
                        .max_w(px(220.))
                        .min_w_0()
                        .overflow_hidden()
                        .whitespace_nowrap()
                        .text_ellipsis_middle()
                        .text_xs()
                        .text_color(muted)
                        .child(detail)
                }))
                .children(keys.map(|keys| {
                    div()
                        .flex_none()
                        .pl_4()
                        .text_xs()
                        .text_color(muted)
                        .child(keys)
                })),
        );
    }
    panel = panel.child(
        div()
            .mt_1()
            .px_2()
            .pt_2()
            .pb_1()
            .border_t_1()
            .border_color(theme.border)
            .text_xs()
            .text_color(theme.text_muted)
            .child("Start with / or ~ for a path, > for a command. Escape goes back."),
    );
    Some(anchor.popup(panel))
}

#[cfg(test)]
mod tests {
    use super::{is_path, scheme_of};

    #[test]
    fn paths_and_schemes_are_told_from_words() {
        for text in ["/srv/data", "~/it's here", "./x"] {
            assert!(is_path(text), "{text}");
        }
        for text in ["report 2026", "it's a draft", "size>10M", "kind:image"] {
            assert!(!is_path(text), "{text}");
        }
        assert_eq!(scheme_of("query:kind:image"), Some("query"));
        assert_eq!(scheme_of("kind:image"), Some("kind"));
        assert_eq!(scheme_of("it's: a draft"), None);
        assert_eq!(scheme_of("report"), None);
    }
}
