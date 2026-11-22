//! What a pane says when it has nothing to list: an empty folder, a search
//! without results, an empty shelf, or a location that could not be opened,
//! with the reason and the way out. Shown in place of the view, inside the
//! pane container, so drops and the folder's menu keep working on it.

use crate::sizes::TextSizes;
use crate::sizes::sized;
use gpui::{AnyElement, Context, div, prelude::*, px};
use skwir_core::command::Invocation;
use skwir_core::location::{EmptyState, Failure, Unshown};
use skwir_core::workspace::PaneId;

use skwir_core::words;
use skwir_core::words::Words;

use crate::controls::{self, ButtonStyle};
use crate::shell::Shell;
use crate::theme::Theme;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Placeholder {
    pub icon: &'static str,
    pub title: String,
    pub detail: String,
    /// Its buttons, in their order: each label and the command it runs.
    pub actions: Vec<(String, Invocation)>,
    pub problem: bool,
}

/// What brings back a folder a pane cannot show: the volume or server it
/// was on that is not open now (not mounted, not connected), as its owner
/// offers to open it again there (`Registry::reopens`, ADR-149).
#[derive(Clone, Debug, PartialEq)]
pub struct Reopening {
    pub title: Words,
    pub label: Words,
    /// What its button runs, the pane and the folder below given.
    pub command: Invocation,
    /// What its owner does to it now ("Mounting…"): said in place of the
    /// button, which is not offered again meanwhile.
    pub busy: Option<Words>,
}

/// A failure in the user's words, by its kind (`Listing::failure`). The
/// system's message stays as the detail: it is what someone will search
/// for. A folder on a place not open now says so, and offers to open it
/// again (`reopen`), then to leave it for home (`home`); one whose volume
/// was let go of while shown, with nothing to open it again, offers home
/// alone.
pub fn failure(
    kind: Failure,
    error: &Words,
    reopen: Option<Reopening>,
    home: Option<(Words, Invocation)>,
) -> Placeholder {
    let placeholder =
        |title: Words, detail: String, actions: Vec<(Words, Invocation)>| Placeholder {
            icon: "warning",
            title: title.say(),
            detail,
            actions: actions
                .into_iter()
                .map(|(label, command)| (label.say(), command))
                .collect(),
            problem: true,
        };
    if kind != Failure::Denied
        && let Some(reopen) = reopen
    {
        let detail = match (&reopen.busy, kind) {
            (Some(busy), _) => busy.say(),
            // Why it could not be opened, the system's words.
            (None, Failure::Other) => error.say(),
            (None, _) => words!(
                placeholder_reopen_detail = "This folder is on it, and shows here again once it is back.",
                note = "Under \"… is not mounted\" or \"… is not connected\" in place of a folder's files: the folder shows again by itself once its drive or server is back, whoever mounts it."
            )
            .say(),
        };
        let actions = reopen
            .busy
            .is_none()
            .then_some((reopen.label, reopen.command))
            .into_iter()
            .chain(home)
            .collect();
        return placeholder(reopen.title, detail, actions);
    }
    let (title, actions) = match kind {
        Failure::Denied => (
            words!(placeholder_denied = "You don't have permission to open this folder"),
            Vec::new(),
        ),
        Failure::Missing => (
            words!(placeholder_gone = "This folder no longer exists"),
            vec![(
                words!(placeholder_up = "Go to the parent folder"),
                Invocation::new("up"),
            )],
        ),
        Failure::Left => (
            words!(
                placeholder_left = "This folder is no longer available",
                note = "Shown in place of a folder's files when the drive or server it is on was unmounted or disconnected while it was shown; the reason follows."
            ),
            home.into_iter().collect(),
        ),
        Failure::Other => (
            words!(placeholder_failed = "This location could not be opened"),
            vec![(
                words!(placeholder_retry = "Try again"),
                Invocation::new("refresh"),
            )],
        ),
    };
    placeholder(title, error.say(), actions)
}

/// What brings back the folder at `path` that `pane` cannot show: the
/// volume or server nearest above it offering to open it again
/// (`Registry::reopens`), its command given the pane and the folder below.
pub(crate) fn reopening(shell: &Shell, pane: PaneId, path: &std::path::Path) -> Option<Reopening> {
    let reopens = shell.app.registry.reopens.all();
    let (reopen, below) = reopens
        .iter()
        .filter_map(|reopen| Some((reopen, path.strip_prefix(&reopen.at).ok()?)))
        .max_by_key(|(reopen, _)| reopen.at.as_os_str().len())?;
    Some(Reopening {
        title: reopen.title.clone(),
        label: reopen.label.clone(),
        command: reopen
            .command
            .clone()
            .with("pane", pane)
            .with("below", skwir_core::os_path::PathArg::from(below)),
        busy: reopen.busy.clone(),
    })
}

/// The way back from a filter that leaves nothing.
fn show_everything() -> String {
    words!(placeholder_show_everything = "Show everything").say()
}

/// A folder with nothing in it: files can be dropped in, or a folder
/// made with `new_folder_keys`.
pub fn empty_folder(new_folder_keys: Option<&str>) -> Placeholder {
    Placeholder {
        icon: "folder-plus",
        title: words!(placeholder_empty = "This folder is empty").say(),
        detail: match new_folder_keys {
            Some(keys) => words!(
                placeholder_drop_or_make = "Drop files here, or press { $keys } for a new folder.",
                keys = skwir_core::words::Keys(keys.to_owned()),
                note = "keys: the keys that make a folder, as the keymap says them (Ctrl+Shift+N)."
            ),
            None => words!(placeholder_drop = "Drop files here."),
        }
        .say(),
        actions: Vec::new(),
        problem: false,
    }
}

/// What is here the program's filter leaves out, in a window answering
/// its dialog (ADR-141, 6): named by the filter (`filter`, as the program
/// calls it), with the way to see every file, the bar's All files
/// (`ui.chooser_filter` given none), named as the bar's list names it:
/// the program's own filter of every file where it sent one (`all`, as it
/// calls it), else Skwir's.
pub fn program_filtered(filter: &str, all: Option<&str>) -> Placeholder {
    Placeholder {
        icon: "search",
        title: words!(
            placeholder_program_filtered = "No files for { $filter } here",
            filter = skwir_core::words::Name(filter.to_owned()),
            note = "In a window answering another program's Open dialog, where the filter that program asked for (filter: its name, as the program gave it: Pictures, JPEG) leaves out every file of the folder."
        )
        .say(),
        detail: match all {
            None => words!(
                placeholder_program_filtered_detail = "The program asks for { $filter }. Choose All files in the bar to see every file.",
                filter = skwir_core::words::Name(filter.to_owned()),
                note = "Under \"No files for … here\"; All files is the filter dropdown's last choice in the bar at the window's foot."
            ),
            Some(all) => words!(
                placeholder_program_filtered_all_detail = "The program asks for { $filter }. Choose { $all } in the bar to see every file.",
                filter = skwir_core::words::Name(filter.to_owned()),
                all = skwir_core::words::Name(all.to_owned()),
                note = "Under \"No files for … here\", where the program gave its own choice of every file (all: its name, as the program gave it: All Files, Any file), which the filter dropdown of the bar at the window's foot lists among its filters."
            ),
        }
        .say(),
        actions: vec![(
            words!(placeholder_show_all_files = "Show all files").say(),
            Invocation::new("ui.chooser_filter"),
        )],
        problem: false,
    }
}

/// What is here is hidden, and hidden files are off.
pub fn only_hidden(show_keys: Option<&str>) -> Placeholder {
    Placeholder {
        icon: "eye-off",
        title: words!(placeholder_hidden = "Only hidden files here").say(),
        detail: match show_keys {
            Some(keys) => words!(
                placeholder_hidden_show_keys = "Show hidden files to see them ({ $keys }).",
                keys = skwir_core::words::Keys(keys.to_owned()),
                note = "keys: the keys that show hidden files (Ctrl+H)."
            ),
            None => words!(placeholder_hidden_show = "Show hidden files to see them."),
        }
        .say(),
        actions: vec![(
            words!(placeholder_show_hidden = "Show hidden files").say(),
            Invocation::new("toggle_hidden"),
        )],
        problem: false,
    }
}

/// What is here keeps other files' notes beside them (ADR-028), and
/// companions are not shown: hidden files alone would show nothing. Its
/// button shows both, and says the setting it turns on is every folder's.
pub fn only_companions() -> Placeholder {
    Placeholder {
        icon: "eye-off",
        title: words!(placeholder_companions = "Only companion files here").say(),
        detail: words!(
            placeholder_companions_detail = "They keep other files' notes: TagSpaces' .ts folder, a Mac's ._ files, XMP sidecars. Showing them turns on Show companion files in Settings (Tags and notes), for every folder.",
            note = "Show companion files, Settings and Tags and notes are the names of a setting and of where it is."
        )
        .say(),
        actions: vec![(
            words!(placeholder_show_companions = "Show hidden and companion files").say(),
            Invocation::new("show_companions"),
        )],
        problem: false,
    }
}

/// A location that is no folder, with nothing to list: what it says of
/// itself (`LocationProvider::empty_state`, read with its glyph in
/// `trail.rs`), never words chosen here by its scheme.
pub fn empty_place(icon: Option<&'static str>, empty: Option<&EmptyState>) -> Placeholder {
    let (title, detail) = empty.map_or_else(
        || {
            (
                words!(placeholder_nothing = "Nothing here").say(),
                String::new(),
            )
        },
        |empty| (empty.title.say(), empty.detail.say()),
    );
    Placeholder {
        icon: icon.unwrap_or("info"),
        title,
        detail,
        actions: Vec::new(),
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
        let reopen = state
            .location()
            .and_then(skwir_core::location::Location::path)
            .and_then(|path| reopening(shell, pane, &path));
        // Home, whatever the pane showed before: a step back may be on
        // the same volume.
        return Some(failure(kind, error, reopen, crate::loading::leaving(false)));
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
            let title = match reading {
                crate::quick_filters::Reading::Counting => {
                    words!(placeholder_reading = "Reading the details of the files…")
                }
                crate::quick_filters::Reading::Files(n) => words!(
                    placeholder_reading_files = "Reading the details of { $count ->
                        [one] one file
                       *[other] { $count } files
                    }…",
                    count = n
                ),
            };
            return Some(Placeholder {
                icon: "search",
                title: title.say(),
                detail: words!(
                    placeholder_reading_detail =
                        "The files { $filter } keeps show as they are read.",
                    filter = skwir_core::words::Name(filter.to_string()),
                    note = "filter: what was typed in the address to filter the folder."
                )
                .say(),
                actions: vec![(show_everything(), Invocation::new("filter"))],
                problem: false,
            });
        }
        // Nothing yet, not nothing: a folder here may still lead to it.
        if state.looks_below() {
            return Some(Placeholder {
                icon: "search",
                title: words!(placeholder_looking = "Looking in the folders here…").say(),
                detail: words!(
                    placeholder_looking_detail =
                        "The folders that lead to { $filter } show as they are found.",
                    filter = skwir_core::words::Name(filter.to_string())
                )
                .say(),
                actions: vec![(show_everything(), Invocation::new("filter"))],
                problem: false,
            });
        }
        let detail = if state.lens().is_some() {
            words!(
                placeholder_lens_empty = "Nothing here is { $filter }. The filters stay as you go to other folders and searches, until you take them off in the address.",
                filter = skwir_core::words::Name(filter.to_string())
            )
        } else if state.read_filter().is_some() {
            words!(
                placeholder_filter_empty = "Nothing here is { $filter }. Press Enter in the address field to search the folders inside too.",
                filter = skwir_core::words::Name(filter.to_string())
            )
        } else {
            words!(
                placeholder_name_empty = "No name here holds { $filter }. Press Enter in the address field to search the folders inside too.",
                filter = skwir_core::words::Name(filter.to_string())
            )
        };
        return Some(Placeholder {
            icon: "search",
            title: words!(placeholder_filtered = "Hidden by filters").say(),
            detail: detail.say(),
            actions: vec![(show_everything(), Invocation::new("filter"))],
            problem: false,
        });
    }
    // Left out by the filter of the program whose dialog the window
    // answers, not by the user's (ADR-141, 6): said, with the way to every
    // file, rather than "empty". Choosing folders, files show dimmed and
    // nothing is left out.
    if state.admits().is_some_and(|admits| !admits.dims)
        && !state.unfiltered_order().is_empty()
        && let Some(chooser) = shell.chooser.as_ref()
        && let Some(filter) = chooser.filter_name()
    {
        return Some(program_filtered(
            &filter,
            chooser.own_all_files().as_deref(),
        ));
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
    if !placeholder.actions.is_empty() {
        let mut buttons = div().mt_2().flex().flex_wrap().justify_center().gap_2();
        for (at, (label, command)) in placeholder.actions.into_iter().enumerate() {
            buttons = buttons.child(
                controls::button(
                    format!("placeholder-action-{at}"),
                    label,
                    None,
                    ButtonStyle::Plain,
                    focus,
                    &theme,
                )
                .on_click(cx.listener(move |this, _, window, cx| {
                    this.run(&command.name, command.args.clone(), window, cx);
                })),
            );
        }
        card = card.child(buttons);
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
    use super::{EmptyState, Reopening, empty_folder, empty_place, failure, program_filtered};
    use skwir_core::command::Invocation;
    use skwir_core::location::Failure;
    use skwir_core::words::Words;

    /// A placeholder's button as its words and its command's name.
    fn button(action: Option<&(String, Invocation)>) -> Option<(&str, &str)> {
        action.map(|(label, command)| (label.as_str(), &*command.name))
    }

    /// Its buttons as their words and their commands' names.
    fn buttons(placeholder: &super::Placeholder) -> Vec<(&str, &str)> {
        placeholder
            .actions
            .iter()
            .map(|(label, command)| (label.as_str(), &*command.name))
            .collect()
    }

    #[test]
    fn failures_read_as_sentences_and_keep_the_system_message() {
        // As a listing says one: its kind from the system's code, its words
        // the system's.
        let of = |error: std::io::Error| {
            let error = skwir_core::ListingError::of_io(&error);
            (
                failure(error.kind, &error.message, None, None),
                error.message,
            )
        };
        let (denied, said) = of(std::io::Error::from(std::io::ErrorKind::PermissionDenied));
        assert_eq!(
            denied.title,
            "You don't have permission to open this folder"
        );
        assert_eq!(denied.detail, said.english());
        assert!(denied.actions.is_empty());
        // A folder gone, as this system says it.
        let (gone, said) = of(std::fs::metadata("/nowhere/it’s gone").unwrap_err());
        assert_eq!(
            button(gone.actions.first()),
            Some(("Go to the parent folder", "up")),
            "{said}"
        );
        #[cfg(unix)]
        for code in [24, 28] {
            // Too many open files; no space left on the device.
            let (other, said) = of(std::io::Error::from_raw_os_error(code));
            assert_eq!(
                button(other.actions.first()),
                Some(("Try again", "refresh")),
                "{said}"
            );
        }
        // Windows' access denied is its code 5, not errno 13.
        #[cfg(windows)]
        assert_eq!(
            of(std::io::Error::from_raw_os_error(5)).0.title,
            "You don't have permission to open this folder"
        );
        // A provider's own words are no system's: said as they are.
        let words =
            skwir_core::ListingError::new(Words::as_written("Could not connect to the server"));
        assert_eq!(
            button(
                failure(words.kind, &words.message, None, None)
                    .actions
                    .first()
            ),
            Some(("Try again", "refresh"))
        );
    }

    #[test]
    fn a_folder_whose_volume_went_says_so_and_how_it_comes_back() {
        // Its drive let go of while shown, nothing to mount it again with:
        // said, the way out home.
        let left = skwir_core::ListingError::left(Words::as_written("The drive went."));
        let home = Some((Words::as_written("Go Home"), Invocation::new("navigate")));
        let said = failure(left.kind, &left.message, None, home.clone());
        assert_eq!(said.title, "This folder is no longer available");
        assert_eq!(said.detail, "The drive went.");
        assert_eq!(buttons(&said), [("Go Home", "navigate")]);
        // Its place offers to mount it again: its words, its button, the
        // command as given (the pane and the folder below added by `of`).
        let mount = Invocation::new("places.mount").with("device", "/dev/sdz1");
        let reopen = Reopening {
            title: Words::as_written("USB is not mounted"),
            label: Words::as_written("Mount USB"),
            command: mount.clone(),
            busy: None,
        };
        for kind in [Failure::Left, Failure::Missing] {
            let said = failure(kind, &left.message, Some(reopen.clone()), home.clone());
            assert_eq!(said.title, "USB is not mounted");
            assert_eq!(
                said.detail,
                "This folder is on it, and shows here again once it is back."
            );
            // Mounting it first, then home.
            assert_eq!(said.actions[0], ("Mount USB".to_owned(), mount.clone()));
            assert_eq!(
                buttons(&said),
                [("Mount USB", "places.mount"), ("Go Home", "navigate")]
            );
        }
        // A connection lost says the system's words, the button the same.
        let lost = skwir_core::ListingError::new(Words::as_written("not connected"));
        let said = failure(lost.kind, &lost.message, Some(reopen.clone()), None);
        assert_eq!(said.detail, "not connected");
        assert_eq!(
            button(said.actions.first()),
            Some(("Mount USB", "places.mount"))
        );
        // While it mounts, what is done, and no button to press again.
        let busy = Reopening {
            busy: Some(Words::as_written("Mounting…")),
            ..reopen.clone()
        };
        let said = failure(Failure::Left, &left.message, Some(busy), home);
        assert_eq!(said.detail, "Mounting…");
        assert_eq!(buttons(&said), [("Go Home", "navigate")]);
        // A folder refused stays refused: no volume to mount says otherwise.
        let denied = failure(Failure::Denied, &left.message, Some(reopen), None);
        assert_eq!(
            denied.title,
            "You don't have permission to open this folder"
        );
    }

    #[test]
    fn empty_states_say_what_to_do_next() {
        let folder = empty_folder(Some("Ctrl+Shift+N"));
        assert_eq!(folder.title, "This folder is empty");
        assert!(folder.detail.contains("Ctrl+Shift+N"));
        // Any other location says what its provider says, under its glyph.
        let said = EmptyState::new(
            Words::as_written("The shelf is empty"),
            Words::as_written("Collect files…"),
        );
        let shelf = empty_place(Some("shelf"), Some(&said));
        assert_eq!(
            (shelf.icon, shelf.title.as_str()),
            ("shelf", "The shelf is empty")
        );
        assert!(!shelf.problem);
        assert_eq!(empty_place(None, None).title, "Nothing here");
        // A program's filter that leaves out every file is named, with
        // the way to every file (ADR-141, 6), never "empty".
        let filtered = program_filtered("Pictures", None);
        assert!(filtered.title.contains("Pictures"));
        assert!(filtered.detail.contains("All files"));
        assert_eq!(
            button(filtered.actions.first()),
            Some(("Show all files", "ui.chooser_filter"))
        );
        // The program's own choice of every file stands for All files in
        // the bar (49426d87): the detail names it as the bar lists it.
        let its_own = program_filtered("Pictures", Some("Any file"));
        assert!(its_own.detail.contains("Any file"));
        assert!(!its_own.detail.contains("All files"));
    }
}
