//! What a pane says when it has nothing to list: an empty folder, a search
//! without results, an empty shelf, or a location that could not be opened,
//! with the reason and the way out. Shown in place of the view, inside the
//! pane container, so drops and the folder's menu keep working on it.

use crate::sizes::TextSizes;
use crate::sizes::sized;
use gpui::{AnyElement, Context, div, prelude::*, px};
use skwir_core::location::{EmptyState, Failure, Unshown};
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

/// A failure in the user's words, by its kind (`Listing::failure`). The
/// system's message stays as the detail: it is what someone will search
/// for.
pub fn failure(kind: Failure, error: &str) -> Placeholder {
    let (title, action) = match kind {
        Failure::Denied => ("You don't have permission to open this folder", None),
        Failure::Missing => (
            "This folder no longer exists",
            Some(("Go to the parent folder", "up")),
        ),
        Failure::Other => (
            "This location could not be opened",
            Some(("Try again", "refresh")),
        ),
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

/// What is here is hidden, and hidden files are off.
pub fn only_hidden(show_keys: Option<&str>) -> Placeholder {
    Placeholder {
        icon: "eye-off",
        title: "Only hidden files here".to_owned(),
        detail: match show_keys {
            Some(keys) => format!("Show hidden files to see them ({keys})."),
            None => "Show hidden files to see them.".to_owned(),
        },
        action: Some(("Show hidden files", "toggle_hidden")),
        problem: false,
    }
}

/// What is here keeps other files' notes beside them (ADR-028), and
/// companions are not shown: hidden files alone would show nothing. Its
/// button shows both, and says the setting it turns on is every folder's.
pub fn only_companions() -> Placeholder {
    Placeholder {
        icon: "eye-off",
        title: "Only companion files here".to_owned(),
        detail: "They keep other files' notes: TagSpaces' .ts folder, a Mac's ._ files, XMP sidecars. Showing them turns on Show companion files in Settings (Tags and notes), for every folder.".to_owned(),
        action: Some(("Show hidden and companion files", "show_companions")),
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

/// `None` while the pane has entries or something arriving to draw (ADR-135:
/// a folder empty, or of hidden files only, that a copy fills shows what
/// comes), is still loading, or shows a view that draws its own states.
pub fn of(shell: &Shell, pane: PaneId) -> Option<Placeholder> {
    let ws = shell.app.workspace.read();
    let state = ws.pane(pane)?;
    let list = &shell.ui().list;
    // Its groups' headings are drawn even when every group is folded
    // (ADR-131, 2): they are the way back to the files, never "empty".
    if !list.sorted.is_empty() || !list.arriving.is_empty() || list.rows.grouped() {
        return None;
    }
    if let (Some(kind), Some(error)) = (state.listing.failure(), state.listing.error()) {
        return Some(failure(kind, error));
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
        // Only a filter that reads the files' metadata waits for it.
        let reads = !shell.ui().place.search && state.read_filter().is_some_and(|r| r.metadata);
        if let Some(reading) = shell.ui().quick_filters.reading(reads) {
            let files = match reading {
                crate::quick_filters::Reading::Counting => "the files".to_owned(),
                crate::quick_filters::Reading::Files(n) => crate::format::count(n, "file"),
            };
            return Some(Placeholder {
                icon: "search",
                title: format!("Reading the details of {files}…"),
                detail: format!("The files “{filter}” keeps show as they are read."),
                action: Some(("Show everything", "filter")),
                problem: false,
            });
        }
        // Nothing yet, not nothing: a folder here may still lead to it.
        if state.looks_below() {
            return Some(Placeholder {
                icon: "search",
                title: "Looking in the folders here…".to_owned(),
                detail: format!("The folders that lead to “{filter}” show as they are found."),
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
    // Nothing shown, nothing filtered out, yet entries there: hidden ones,
    // or only files that keep others' notes (ADR-028), which hidden files
    // alone do not show. Said, with the way to see them, rather than what
    // the place says empty (a tag's place listing only a hidden folder, a
    // folder of dot files, one holding only `TagSpaces`' `.ts`).
    match list.unshown {
        Unshown::Hidden => {
            let keys = shell.keys_for("toggle_hidden", &serde_json::Value::Null);
            return Some(only_hidden(keys.as_deref()));
        }
        Unshown::Companions => return Some(only_companions()),
        Unshown::Nothing => {}
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
    // As wide as the pane, up to a measure: narrower, it wraps rather than
    // standing past the pane's edges.
    let mut card = div()
        .w_full()
        .max_w(sized(420.))
        .min_w_0()
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
        // A title by its weight, at the size of the words under it.
        .child(
            div()
                .text_body()
                .font_weight(gpui::FontWeight::SEMIBOLD)
                .text_color(theme.text)
                .child(placeholder.title),
        )
        .when(!placeholder.detail.is_empty(), |d| {
            d.child(
                div()
                    .text_body()
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
        // As a listing says one: its kind from the system's code, its words
        // the system's.
        let of = |error: std::io::Error| {
            let error = skwir_core::ListingError::of_io(&error);
            (failure(error.kind, &error.message), error.message)
        };
        let (denied, said) = of(std::io::Error::from(std::io::ErrorKind::PermissionDenied));
        assert_eq!(
            denied.title,
            "You don't have permission to open this folder"
        );
        assert_eq!(denied.detail, said);
        assert_eq!(denied.action, None);
        // A folder gone, as this system says it.
        let (gone, said) = of(std::fs::metadata("/nowhere/it’s gone").unwrap_err());
        assert_eq!(
            gone.action,
            Some(("Go to the parent folder", "up")),
            "{said}"
        );
        #[cfg(unix)]
        for code in [24, 28] {
            // Too many open files; no space left on the device.
            let (other, said) = of(std::io::Error::from_raw_os_error(code));
            assert_eq!(other.action, Some(("Try again", "refresh")), "{said}");
        }
        // Windows' access denied is its code 5, not errno 13.
        #[cfg(windows)]
        assert_eq!(
            of(std::io::Error::from_raw_os_error(5)).0.title,
            "You don't have permission to open this folder"
        );
        // A provider's own words are no system's: said as they are.
        let words = skwir_core::ListingError::new("Could not connect to the server");
        assert_eq!(
            failure(words.kind, &words.message).action,
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
