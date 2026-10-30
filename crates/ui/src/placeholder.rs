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
    use std::io::ErrorKind;
    let lower = error.to_lowercase();
    // The system's code says what failed, read by the system's own table
    // (errno here, Windows' codes there); a message without one by its
    // words.
    let kind = os_error_kind(error);
    let (title, action) = if kind.map_or(lower.contains("permission denied"), |kind| {
        kind == ErrorKind::PermissionDenied
    }) {
        ("You don't have permission to open this folder", None)
    } else if kind.map_or(lower.contains("no such file"), |kind| {
        matches!(kind, ErrorKind::NotFound | ErrorKind::NotADirectory)
    }) {
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
    // Filtered down to nothing: say so, and the way back. Read by the
    // query plugin, the words are more than names (ADR-098), and its lens
    // stays as the pane goes elsewhere (ADR-107). A file whose details
    // are still read may yet be shown.
    // A location with nothing to hide says what it says empty.
    if let Some(filter) = state
        .filter()
        .filter(|_| !state.unfiltered_order().is_empty())
    {
        let reading = shell.ui().quick_filters.files.unknown;
        if reading > 0 {
            return Some(Placeholder {
                icon: "search",
                title: format!(
                    "Reading the details of {}…",
                    crate::format::count(reading, "file")
                ),
                detail: format!("The files “{filter}” keeps show as they are read."),
                action: Some(("Show everything", "filter")),
                problem: false,
            });
        }
        let detail = if state.lens().is_some() {
            format!(
                "Nothing here is “{filter}”. The filters stay as you go to other folders and searches, until you take them off in the address."
            )
        } else if state.read_filter().is_some() {
            format!(
                "Nothing here is “{filter}”. Press Enter in the address field to search the folders inside too."
            )
        } else {
            format!(
                "No name here holds “{filter}”. Press Enter in the address field to search the folders inside too."
            )
        };
        return Some(Placeholder {
            icon: "search",
            title: "Hidden by filters".to_owned(),
            detail,
            action: Some(("Show everything", "filter")),
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

/// The kind of the error a message ends by naming (`… (os error 2)`, as
/// `std::io::Error` writes one).
fn os_error_kind(error: &str) -> Option<std::io::ErrorKind> {
    let (_, after) = error.rsplit_once("(os error ")?;
    let code = after.split_once(')')?.0.parse().ok()?;
    Some(std::io::Error::from_raw_os_error(code).kind())
}

#[cfg(test)]
mod tests {
    use super::{EmptyState, empty_folder, empty_place, failure};

    #[test]
    fn failures_read_as_sentences_and_keep_the_system_message() {
        let said = std::io::Error::from(std::io::ErrorKind::PermissionDenied).to_string();
        let denied = failure(&said);
        assert_eq!(
            denied.title,
            "You don't have permission to open this folder"
        );
        assert_eq!(denied.detail, said);
        assert_eq!(denied.action, None);
        assert_eq!(
            failure("No such file or directory (os error 2)").action,
            Some(("Go to the parent folder", "up"))
        );
        // Errors as this system writes them, read by their code: a folder
        // gone, and never a number inside a larger one.
        let gone = std::fs::metadata("/nowhere/it’s gone")
            .unwrap_err()
            .to_string();
        assert_eq!(
            failure(&gone).action,
            Some(("Go to the parent folder", "up")),
            "{gone}"
        );
        #[cfg(unix)]
        for code in [24, 28] {
            // Too many open files; no space left on the device.
            let said = std::io::Error::from_raw_os_error(code).to_string();
            assert_eq!(
                failure(&said).action,
                Some(("Try again", "refresh")),
                "{said}"
            );
        }
        // Windows' access denied is its code 5, not errno 13.
        #[cfg(windows)]
        assert_eq!(
            failure(&std::io::Error::from_raw_os_error(5).to_string()).title,
            "You don't have permission to open this folder"
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
