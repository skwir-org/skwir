//! What a pane says when it has nothing to list: an empty folder, a search
//! without results, an empty shelf, or a location that could not be opened,
//! with the reason and the way out. Shown in place of the view, inside the
//! pane container, so drops and the folder's menu keep working on it.

use gpui::{AnyElement, Context, div, prelude::*, px};
use skwir_core::location::EmptyState;
use skwir_core::workspace::PaneId;

use crate::controls::{self, ButtonStyle};
use crate::shell::Shell;
use crate::theme::Theme;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Placeholder {
    pub icon: &'static str,
    pub title: String,
    pub detail: String,
    /// Button label and the command it runs.
    pub action: Option<(&'static str, &'static str)>,
    pub problem: bool,
}

/// A failure in the user's words. The system's message stays as the detail:
/// it is what someone will search for.
pub fn failure(error: &str) -> Placeholder {
    let lower = error.to_lowercase();
    let (title, action) = if lower.contains("permission denied") || lower.contains("os error 13") {
        ("You don't have permission to open this folder", None)
    } else if lower.contains("no such file") || lower.contains("os error 2") {
        (
            "This folder no longer exists",
            Some(("Go to the parent folder", "up")),
        )
    } else if lower.contains("no provider") {
        ("skwir cannot open this kind of location", None)
    } else {
        (
            "This location could not be opened",
            Some(("Try again", "refresh")),
        )
    };
    Placeholder {
        icon: "warning",
        title: title.to_owned(),
        detail: error.to_owned(),
        action,
        problem: true,
    }
}

/// A folder with nothing in it: files can be dropped in, or a folder
/// made with `new_folder_keys`.
pub fn empty_folder(new_folder_keys: Option<&str>) -> Placeholder {
    Placeholder {
        icon: "folder-plus",
        title: "This folder is empty".to_owned(),
        detail: match new_folder_keys {
            Some(keys) => format!("Drop files here, or press {keys} for a new folder."),
            None => "Drop files here.".to_owned(),
        },
        action: None,
        problem: false,
    }
}

/// A location that is no folder, with nothing to list: what it says of
/// itself (`LocationProvider::empty_state`, read with its glyph in
/// `trail.rs`), never words chosen here by its scheme.
pub fn empty_place(icon: Option<&'static str>, empty: Option<&EmptyState>) -> Placeholder {
    let (title, detail) = empty.map_or_else(
        || ("Nothing here".to_owned(), String::new()),
        |empty| (empty.title.clone(), empty.detail.clone()),
    );
    Placeholder {
        icon: icon.unwrap_or("info"),
        title,
        detail,
        action: None,
        problem: false,
    }
}

/// `None` while the pane has entries, is still loading, or shows a view
/// that draws its own states.
pub fn of(shell: &Shell, pane: PaneId) -> Option<Placeholder> {
    let ws = shell.app.workspace.read();
    let state = ws.pane(pane)?;
    if !shell.ui().list.sorted.is_empty() {
        return None;
    }
    if let Some(error) = state.listing.error() {
        return Some(failure(error));
    }
    if !state.listing.is_complete() {
        return None;
    }
    // Filtered by name down to nothing: say so, and the way back.
    if let Some(filter) = state.filter() {
        return Some(Placeholder {
            icon: "search",
            title: format!("No name here holds “{filter}”"),
            detail: "Press Enter in the address field to search the folders inside too.".to_owned(),
            action: Some(("Show everything", "filter")),
            problem: false,
        });
    }
    // Narrowed down to nothing (`query.narrow`): say to what, and the way
    // back.
    if let Some(narrowed) = state.narrowed() {
        let to: Vec<&str> = narrowed.pills.iter().map(|p| p.label.as_str()).collect();
        return Some(Placeholder {
            icon: "sliders",
            title: "No file here is what the folder is narrowed to".to_owned(),
            detail: format!(
                "Narrowed to {}. Take one off in the address, or search the folders inside.",
                to.join(", ")
            ),
            action: Some(("Show everything", "query.narrow")),
            problem: false,
        });
    }
    if state.location()?.is_path() {
        let keys = shell.keys_for("ui.mkdir_prompt", &serde_json::Value::Null);
        return Some(empty_folder(keys.as_deref()));
    }
    let shown = &shell.ui().place;
    Some(empty_place(shown.icon, shown.empty.as_ref()))
}

pub fn render(
    placeholder: Placeholder,
    focus: &gpui::FocusHandle,
    cx: &mut Context<Shell>,
) -> AnyElement {
    let theme = cx.global::<Theme>().clone();
    let color = if placeholder.problem {
        theme.error
    } else {
        theme.text_muted
    };
    let mut card = div()
        .max_w(px(420.))
        .flex()
        .flex_col()
        .items_center()
        .gap_2()
        .text_center()
        .child(
            div()
                .size(px(44.))
                .mb_1()
                .flex()
                .items_center()
                .justify_center()
                .rounded_full()
                .bg(theme.surface)
                .child(controls::glyph(placeholder.icon, color, 22.)),
        )
        .child(div().text_color(theme.text).child(placeholder.title))
        .when(!placeholder.detail.is_empty(), |d| {
            d.child(
                div()
                    .text_xs()
                    .text_color(theme.text_muted)
                    .child(placeholder.detail),
            )
        });
    if let Some((label, command)) = placeholder.action {
        card = card.child(
            controls::button(
                "placeholder-action",
                label,
                None,
                ButtonStyle::Plain,
                focus,
                &theme,
            )
            .mt_2()
            .on_click(cx.listener(move |this, _, window, cx| {
                this.run(command, serde_json::json!({}), window, cx);
            })),
        );
    }
    div()
        .size_full()
        .flex()
        .items_center()
        .justify_center()
        .p_6()
        .child(card)
        .into_any_element()
}

#[cfg(test)]
mod tests {
    use super::{EmptyState, empty_folder, empty_place, failure};

    #[test]
    fn failures_read_as_sentences_and_keep_the_system_message() {
        let denied = failure("Permission denied (os error 13)");
        assert_eq!(
            denied.title,
            "You don't have permission to open this folder"
        );
        assert_eq!(denied.detail, "Permission denied (os error 13)");
        assert_eq!(denied.action, None);
        assert_eq!(
            failure("No such file or directory (os error 2)").action,
            Some(("Go to the parent folder", "up"))
        );
        assert_eq!(
            failure("Input/output error (os error 5)").action,
            Some(("Try again", "refresh"))
        );
    }

    #[test]
    fn empty_states_say_what_to_do_next() {
        let folder = empty_folder(Some("Ctrl+Shift+N"));
        assert_eq!(folder.title, "This folder is empty");
        assert!(folder.detail.contains("Ctrl+Shift+N"));
        // Any other location says what its provider says, under its glyph.
        let said = EmptyState::new("The shelf is empty", "Collect files…");
        let shelf = empty_place(Some("shelf"), Some(&said));
        assert_eq!(
            (shelf.icon, shelf.title.as_str()),
            ("shelf", "The shelf is empty")
        );
        assert!(!shelf.problem);
        assert_eq!(empty_place(None, None).title, "Nothing here");
    }
}
