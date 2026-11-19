//! Another program's Open or Save dialog, answered in a whole window
//! (ADR-141, 4): the window chooses, and a bar at its foot holds what the
//! request asks (the name typed, the program's filters and choices,
//! Cancel and its answer). Only the user answers: the bar's commands are
//! the window's own, the user's alone (`ui.chooser_*`, ADR-044); Enter
//! on a file and a double click answer through `open`, which the window
//! takes while it chooses (`Shell::chooser_open`).

use std::collections::HashSet;
use std::ffi::{OsStr, OsString};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use gpui::{
    AnyElement, Context, Entity, FocusHandle, Focusable, Subscription, Window, div, prelude::*,
};
use skwir_core::Location;
use skwir_core::words;
use skwir_core::words::Words;
use skwir_core::workspace::{Admits, PaneId};
use skwir_platform::chooser::{Answer, Asked, Choosing, Kind};

use crate::controls::{self, ButtonStyle};
use crate::elements::popover::PopoverAnchor;
use crate::input::{InputEvent, TextInput};
use crate::keymap;
use crate::modal;
use crate::shell::Shell;
use crate::sizes::{TextSizes, sized};
use crate::theme::Theme;

/// The folder each program last chose in (ADR-141, 7), by its id, the
/// host's programs (no id) under "".
pub(crate) const FOLDERS_KEY: &str = "ui.chooser_folders";

/// Where the window starts: the request's folder, else the one its
/// program last chose in, else home (ADR-141, 7). Nothing is read: a
/// folder gone says so in the pane, as any.
pub(crate) fn start_folder(app: &skwir_core::App, asked: &Asked) -> PathBuf {
    // A folder the request names that is no folder's path (a relative
    // one) is passed over, as one it does not name.
    if let Some(folder) = asked.folder.as_ref().filter(|folder| folder.is_absolute()) {
        return folder.clone();
    }
    let kept = app
        .settings
        .get::<toml::Table>(FOLDERS_KEY)
        .and_then(|folders| {
            folders
                .get(&asked.app_id)
                .and_then(toml::Value::as_str)
                .map(PathBuf::from)
        })
        .filter(|folder| folder.is_absolute());
    kept.unwrap_or_else(|| skwir_core::os_path::home().unwrap_or_else(|| PathBuf::from("/")))
}

/// The longest handle of a Wayland window taken (ADR-141, 9): the one
/// xdg-foreign exports is a short token (a UUID's length), and what the
/// program gives goes whole into a Wayland request, which libwayland
/// cannot send past its buffer: the display would fail, and every window
/// of this skwir with it.
const HANDLE_MAX: usize = 256;

/// The asking program's window (ADR-141, 9), as the portal names it:
/// `x11:` and its id in hexadecimal, `wayland:` and the handle it
/// exported, printable ASCII of `HANDLE_MAX` bytes at most. Anything else
/// names none: the window is one of its own.
pub(crate) fn foreign_parent(window: &str) -> Option<gpui::ForeignParent> {
    if let Some(id) = window.strip_prefix("x11:") {
        let id = id.strip_prefix("0x").unwrap_or(id);
        return u32::from_str_radix(id, 16)
            .ok()
            .filter(|id| *id != 0)
            .map(gpui::ForeignParent::X11);
    }
    let handle = window.strip_prefix("wayland:")?;
    (!handle.is_empty()
        && handle.len() <= HANDLE_MAX
        && handle.bytes().all(|byte| byte.is_ascii_graphic()))
    .then(|| gpui::ForeignParent::Wayland(handle.to_owned()))
}

/// A request being answered in this window.
pub(crate) struct Chooser {
    pub(crate) choosing: Choosing,
    /// The name typed (saving one file).
    name: Option<Entity<TextInput>>,
    /// The name as bytes the field says (the file saved again's, a file
    /// chosen): answered as they are while its text still says them, a
    /// name not UTF-8 shown in its lossy form (`save_name`).
    given: Option<OsString>,
    /// The filter in use: one of the request's, or all files (`None`).
    filter: Option<usize>,
    /// What it admits, set on the pane shown (`Shell::chooser_sync`).
    admits: Option<Admits>,
    /// The panes it was set on, taken off as the window lets go of the
    /// request or the pane leaves the window.
    admitted: HashSet<PaneId>,
    /// Each choice's value, in the request's order.
    choices: Vec<String>,
    /// The dropdown whose list is open.
    open: Option<Dropdown>,
    filter_focus: FocusHandle,
    filter_anchor: PopoverAnchor,
    choice_focus: Vec<FocusHandle>,
    choice_anchors: Vec<PopoverAnchor>,
    cancel_focus: FocusHandle,
    accept_focus: FocusHandle,
    /// Counts the answers looked into (whether names are taken): one
    /// answered after another was asked is dropped.
    asking: u64,
    _name_events: Option<Subscription>,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Dropdown {
    Filter,
    Choice(usize),
}

impl Chooser {
    fn asked(&self) -> &Asked {
        self.choosing.asked()
    }

    /// The choices as set, as the answer gives them (id, value).
    fn choices_set(&self) -> Vec<(String, String)> {
        self.asked()
            .choices
            .iter()
            .zip(&self.choices)
            .map(|(choice, value)| (choice.id.clone(), value.clone()))
            .collect()
    }
}

/// What the filter at `filter` admits, folders always (`Admits`): while
/// choosing folders, no file.
fn admits_of(asked: &Asked, filter: Option<usize>) -> Option<Admits> {
    if matches!(
        asked.kind,
        Kind::Open {
            directory: true,
            ..
        }
    ) {
        return Some(Admits(Arc::new(|_| false)));
    }
    let filter = asked.filters.get(filter?)?;
    Some(Admits(Arc::new(filter.admits())))
}

/// The name to save under, of the field's `text`: none while it is empty
/// (or spaces alone); `given`'s bytes while the text still says them (a
/// name not UTF-8 is shown in its lossy form), else the text as typed,
/// its spaces kept (a name may begin or end with one).
fn save_name(text: &str, given: Option<&OsStr>) -> Option<OsString> {
    if text.trim().is_empty() {
        return None;
    }
    Some(match given {
        Some(given) if given.to_string_lossy() == text => given.to_os_string(),
        _ => OsString::from(text),
    })
}

/// How much of `name` is its stem: what is selected to be typed over.
fn stem_len(name: &str) -> usize {
    match name.rfind('.') {
        Some(0) | None => name.len(),
        Some(dot) => dot,
    }
}

/// The window's title: the request's, else what it asks.
pub(crate) fn title(asked: &Asked) -> Words {
    if !asked.title.trim().is_empty() {
        return Words::as_written(asked.title.clone());
    }
    match asked.kind {
        Kind::Open {
            directory: true, ..
        } => words!(
            chooser_title_folder = "Choose a folder",
            note = "The title of the window another program opened to choose a folder, where it gave none."
        ),
        Kind::Open { .. } => words!(
            chooser_title_open = "Open",
            note = "The title of the window another program opened to choose files to open, where it gave none."
        ),
        Kind::Save { .. } | Kind::SaveFiles { .. } => words!(
            chooser_title_save = "Save",
            note = "The title of the window another program opened to choose where to save, where it gave none."
        ),
    }
}

/// The answer's button: the program's words, else what it does.
fn accept_label(asked: &Asked) -> Words {
    if let Some(label) = &asked.accept_label {
        // A label written with a mnemonic (GTK's `_Open`, `__` for an
        // underscore) says it plainly.
        let plain: Vec<String> = label
            .split("__")
            .map(|part| part.replace('_', ""))
            .collect();
        return Words::as_written(plain.join("_"));
    }
    match asked.kind {
        Kind::Open {
            directory: true, ..
        } => words!(
            chooser_accept_folder = "Select",
            note = "A button of the bar answering another program's dialog: choose the folder."
        ),
        Kind::Open { .. } => words!(
            chooser_accept_open = "Open",
            note = "A button of the bar answering another program's dialog: open the files chosen."
        ),
        Kind::Save { .. } | Kind::SaveFiles { .. } => words!(
            chooser_accept_save = "Save",
            note = "A button of the bar answering another program's dialog: save there."
        ),
    }
}

fn all_files() -> Words {
    words!(
        chooser_all_files = "All files",
        note = "In the filter dropdown of the bar answering another program's dialog: show every file."
    )
}

impl Shell {
    /// This window answers `choosing` (ADR-141): the bar shown, the name
    /// to save under typed in and its stem selected, the program's
    /// filter in use. The pane shows where it starts (`start_folder`).
    pub(crate) fn answer_dialog(
        &mut self,
        choosing: Choosing,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let asked = choosing.asked().clone();
        let given = asked.save_name().filter(|name| !name.is_empty());
        let name = matches!(asked.kind, Kind::Save { .. }).then(|| {
            let start = given
                .as_deref()
                .map(|name| name.to_string_lossy().into_owned())
                .unwrap_or_default();
            cx.new(|cx| {
                let mut input = TextInput::new(
                    words!(
                        chooser_name_placeholder = "Name of the file",
                        note = "In the empty name field of the bar answering another program's Save dialog."
                    ),
                    cx,
                );
                input.set_text(start.clone(), cx);
                input.select_range(0..stem_len(&start), cx);
                input
            })
        });
        let events = name.as_ref().map(|input| {
            cx.subscribe_in(
                input,
                window,
                |this, _, event: &InputEvent, window, cx| match event {
                    InputEvent::Submit => this.chooser_accept(window, cx),
                    InputEvent::Cancel => this.chooser_cancel(window, cx),
                    InputEvent::Changed => {}
                },
            )
        });
        let filter = asked.first_filter();
        let choices = asked
            .choices
            .iter()
            .map(
                |choice| match (choice.is_switch(), choice.initial.as_str()) {
                    (true, "true") => "true".to_owned(),
                    (true, _) => "false".to_owned(),
                    (false, initial) if choice.options.iter().any(|(id, _)| id == initial) => {
                        initial.to_owned()
                    }
                    (false, _) => choice
                        .options
                        .first()
                        .map(|(id, _)| id.clone())
                        .unwrap_or_default(),
                },
            )
            .collect();
        let count = asked.choices.len();
        // The program gave up: the window goes at the next wake.
        let wake = self.app.waker_handle();
        choosing.when_closed(move || wake());
        self.chooser = Some(Chooser {
            admits: admits_of(&asked, filter),
            admitted: HashSet::new(),
            choosing,
            name,
            filter,
            choices,
            open: None,
            filter_focus: cx.focus_handle(),
            filter_anchor: PopoverAnchor::default(),
            choice_focus: (0..count).map(|_| cx.focus_handle()).collect(),
            choice_anchors: (0..count).map(|_| PopoverAnchor::default()).collect(),
            cancel_focus: cx.focus_handle(),
            accept_focus: cx.focus_handle(),
            asking: 0,
            given,
            _name_events: events,
        });
        self.chooser_sync(window, cx);
        cx.notify();
    }

    /// Saving one file, the name it is saved under takes the keyboard (as
    /// the window opens): whether it did.
    pub(crate) fn chooser_focus(&self, window: &mut Window, cx: &mut Context<Self>) -> bool {
        let Some(input) = self.chooser.as_ref().and_then(|c| c.name.clone()) else {
            return false;
        };
        let focus = input.read(cx).focus_handle(cx);
        window.focus(&focus, cx);
        true
    }

    /// What the window keeps of its request as it syncs: the filter in use
    /// on the pane shown (another tab's too, once shown), and the window
    /// gone once the program gave up.
    pub(crate) fn chooser_sync(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(chooser) = &self.chooser else {
            return;
        };
        if chooser.choosing.is_closed() {
            tracing::info!("the program gave up its dialog");
            self.chooser_end(Answer::Cancelled, window, cx);
            return;
        }
        let admits = chooser.admits.clone();
        let (pane, window_id) = (self.pane, self.window_id);
        let mut ws = self.app.workspace.write();
        if let Some(shown) = ws.pane_mut(pane) {
            shown.set_admits(admits);
        }
        // A pane moved to another window shows all again there.
        let Some(chooser) = &mut self.chooser else {
            return;
        };
        chooser.admitted.insert(pane);
        chooser.admitted.retain(|admitted| {
            let here = ws.window_of(*admitted) == Some(window_id);
            if !here && let Some(moved) = ws.pane_mut(*admitted) {
                moved.set_admits(None);
            }
            here
        });
    }

    /// The window answers `answer` (unless it did) and goes from the
    /// core, whatever else is there: never kept as the last window, nor
    /// an ordinary one (ADR-141). What it admitted goes with it. Every way
    /// the window goes passes here (`Shell::leave`). Nothing if it chooses
    /// nothing.
    pub(crate) fn chooser_let_go(&mut self, answer: Answer, cx: &mut gpui::App) {
        let Some(chooser) = self.chooser.take() else {
            return;
        };
        if let Answer::Chosen { paths, .. } = &answer {
            self.chooser_remember(chooser.asked(), paths);
        }
        chooser.choosing.answer(answer);
        {
            let mut ws = self.app.workspace.write();
            for admitted in &chooser.admitted {
                if let Some(pane) = ws.pane_mut(*admitted) {
                    pane.set_admits(None);
                }
            }
        }
        // Gone already where the core closed it.
        if self.app.workspace.read().window(self.window_id).is_none() {
            return;
        }
        let home = skwir_core::os_path::home().unwrap_or_else(|| PathBuf::from("/"));
        let folder = skwir_core::settings::startup_folder(&self.app.settings, &home);
        match self.app.close_dialog_window(self.window_id, &folder) {
            // The core's last, a window of the user's own took its place:
            // asleep until one is asked, as the others.
            Ok(Some(placed)) => crate::finishing::sleeps(cx, placed),
            Ok(None) => {}
            Err(error) => tracing::warn!("a dialog's window closes: {error:#}"),
        }
    }

    /// `chooser_let_go`, and the window closes.
    fn chooser_end(&mut self, answer: Answer, window: &mut Window, cx: &mut Context<Self>) {
        self.chooser_let_go(answer, cx);
        self.leave(window, cx);
        window.remove_window();
    }

    /// The folder its program chose in, kept for its next dialog
    /// (ADR-141, 7): the one shown, else the first path's.
    fn chooser_remember(&self, asked: &Asked, paths: &[PathBuf]) {
        let folder = self.app.pane_dir(self.pane).ok().or_else(|| {
            paths
                .first()
                .and_then(|p| p.parent())
                .map(Path::to_path_buf)
        });
        let Some(folder) = folder.and_then(|f| f.to_str().map(str::to_owned)) else {
            return;
        };
        let mut folders = self
            .app
            .settings
            .get::<toml::Table>(FOLDERS_KEY)
            .unwrap_or_default();
        if folders.get(&asked.app_id).and_then(toml::Value::as_str) == Some(folder.as_str()) {
            return;
        }
        folders.insert(asked.app_id.clone(), toml::Value::String(folder));
        self.app
            .settings
            .set(FOLDERS_KEY, toml::Value::Table(folders));
        self.save_settings_later();
    }

    /// Cancel: the program is told nothing was chosen.
    pub(crate) fn chooser_cancel(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.chooser.is_some() {
            self.chooser_end(Answer::Cancelled, window, cx);
        }
    }

    /// The paths answered with what is set beside them.
    fn chooser_chosen(&self, paths: Vec<PathBuf>) -> Answer {
        let Some(chooser) = &self.chooser else {
            return Answer::Cancelled;
        };
        Answer::Chosen {
            paths,
            choices: chooser.choices_set(),
            filter: chooser
                .filter
                .and_then(|at| chooser.asked().filters.get(at).cloned()),
        }
    }

    /// What is selected in the pane, each with whether it is a folder, the
    /// cursor's first.
    fn chooser_selected(&self) -> Vec<(PathBuf, bool)> {
        let ws = self.app.workspace.read();
        let Some(p) = ws.pane(self.pane) else {
            return Vec::new();
        };
        let cursor = p.selection.cursor.filter(|id| p.selection.contains(*id));
        cursor
            .into_iter()
            .chain(p.selection.iter().filter(|id| Some(*id) != cursor))
            .filter_map(|id| Some((p.path_of(id)?, p.entry(id)?.is_dir())))
            .collect()
    }

    /// The answer's button, or Enter in the name: what the request asks,
    /// chosen; a name taken asked Replace? first.
    pub(crate) fn chooser_accept(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(chooser) = &self.chooser else {
            return;
        };
        let kind = chooser.asked().kind.clone();
        match kind {
            Kind::Open {
                multiple,
                directory: false,
            } => {
                let files: Vec<PathBuf> = self
                    .chooser_selected()
                    .into_iter()
                    .filter(|(_, dir)| !dir)
                    .map(|(path, _)| path)
                    .take(if multiple { usize::MAX } else { 1 })
                    .collect();
                if files.is_empty() {
                    self.set_status(
                        words!(
                            chooser_choose_file = "Choose a file first",
                            note = "Said where the bar answering another program's Open dialog was pressed with no file selected."
                        ),
                        true,
                        cx,
                    );
                    return;
                }
                let answer = self.chooser_chosen(files);
                self.chooser_end(answer, window, cx);
            }
            Kind::Open {
                multiple,
                directory: true,
            } => {
                let mut folders: Vec<PathBuf> = self
                    .chooser_selected()
                    .into_iter()
                    .filter(|(_, dir)| *dir)
                    .map(|(path, _)| path)
                    .take(if multiple { usize::MAX } else { 1 })
                    .collect();
                if folders.is_empty()
                    && let Ok(shown) = self.app.pane_dir(self.pane)
                {
                    folders.push(shown);
                }
                if folders.is_empty() {
                    self.set_status(
                        words!(
                            chooser_choose_folder = "Choose a folder first",
                            note = "Said where the bar answering another program's dialog for a folder was pressed with none selected or shown."
                        ),
                        true,
                        cx,
                    );
                    return;
                }
                let answer = self.chooser_chosen(folders);
                self.chooser_end(answer, window, cx);
            }
            Kind::Save { .. } => self.chooser_save(window, cx),
            Kind::SaveFiles { files } => self.chooser_save_files(files, window, cx),
        }
    }

    /// The folder saved in: the one shown, else none (a search shows no
    /// folder to save in, ADR-141, 5).
    fn chooser_folder(&mut self, cx: &mut Context<Self>) -> Option<PathBuf> {
        let folder = self.app.pane_dir(self.pane).ok();
        if folder.is_none() {
            self.set_status(
                words!(
                    chooser_save_in_folder = "Go to a folder to save in",
                    note = "Said where the bar answering another program's Save dialog was pressed while the pane shows a search or a tag, not a folder."
                ),
                true,
                cx,
            );
        }
        folder
    }

    /// Save one file: under the name typed, in the folder shown; a folder
    /// typed is gone into, a name taken asked Replace? first. Whether a
    /// name is taken is read on a worker, in the folder's lane.
    fn chooser_save(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let typed = self.chooser.as_ref().and_then(|c| {
            let text = c.name.as_ref()?.read(cx).text().to_owned();
            save_name(&text, c.given.as_deref())
        });
        let Some(typed) = typed else {
            self.set_status(
                words!(
                    chooser_type_name = "Type a name to save under",
                    note = "Said where the bar answering another program's Save dialog was pressed with its name field empty."
                ),
                true,
                cx,
            );
            return;
        };
        let Some(folder) = self.chooser_folder(cx) else {
            return;
        };
        self.chooser_save_at(folder.join(typed), window, cx);
    }

    /// Save one file at `path`: a folder there is gone into, a name taken
    /// asked Replace? first, read on a worker in its folder's lane.
    fn chooser_save_at(&mut self, path: PathBuf, window: &mut Window, cx: &mut Context<Self>) {
        let asking = self.chooser_asks();
        self.off_thread(
            skwir_core::fs::reach::lane(&path),
            "ui-chooser-name",
            {
                let path = path.clone();
                move || {
                    (
                        std::fs::symlink_metadata(&path).ok().map(|meta| {
                            meta.is_dir() || std::fs::metadata(&path).is_ok_and(|m| m.is_dir())
                        }),
                        path.parent().is_some_and(Path::is_dir),
                    )
                }
            },
            move |this, (taken, parent), window, cx| {
                if this.chooser.as_ref().is_none_or(|c| c.asking != asking) {
                    return;
                }
                match taken {
                    // A folder typed: gone into, the name left to type.
                    Some(true) => {
                        let _ = this.app.navigate(this.pane, &Location::from_path(path));
                        if let Some(input) = this.chooser.as_ref().and_then(|c| c.name.clone()) {
                            input.update(cx, |input, cx| input.set_text("", cx));
                        }
                    }
                    Some(false) => {
                        let name = path
                            .file_name()
                            .map(|n| n.to_string_lossy().into_owned())
                            .unwrap_or_default();
                        let folder = path
                            .parent()
                            .map(|p| p.display().to_string())
                            .unwrap_or_default();
                        this.open_confirm(
                            words!(
                                chooser_replace = "Replace { $name }? A file of that name is already in { $folder }.",
                                name = name,
                                folder = folder,
                                note = "Asked before another program saves over a file: name: the file's name; folder: where it is."
                            ),
                            crate::prompts::Confirmed::Replace(vec![path]),
                            window,
                            cx,
                        );
                    }
                    None if !parent => this.set_status(
                        words!(
                            chooser_no_folder = "There is no folder { $folder }",
                            folder = path
                                .parent()
                                .map(|p| p.display().to_string())
                                .unwrap_or_default(),
                            note = "Said where a name typed in another program's Save dialog names a folder that does not exist."
                        ),
                        true,
                        cx,
                    ),
                    None => {
                        let answer = this.chooser_chosen(vec![path]);
                        this.chooser_end(answer, window, cx);
                    }
                }
            },
            window,
            cx,
        );
    }

    /// Save several files, each under its own name, in the folder chosen
    /// (the one selected, else the one shown); names taken asked Replace?
    /// once for all.
    fn chooser_save_files(
        &mut self,
        files: Vec<std::ffi::OsString>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let selected: Vec<PathBuf> = self
            .chooser_selected()
            .into_iter()
            .filter(|(_, dir)| *dir)
            .map(|(path, _)| path)
            .collect();
        let folder = match <[PathBuf; 1]>::try_from(selected) {
            Ok([folder]) => folder,
            Err(_) => match self.chooser_folder(cx) {
                Some(folder) => folder,
                None => return,
            },
        };
        let paths: Vec<PathBuf> = files.iter().map(|name| folder.join(name)).collect();
        let asking = self.chooser_asks();
        self.off_thread(
            skwir_core::fs::reach::lane(&folder),
            "ui-chooser-names",
            {
                let paths = paths.clone();
                move || {
                    paths
                        .iter()
                        .filter(|path| std::fs::symlink_metadata(path).is_ok())
                        .count()
                }
            },
            move |this, taken, window, cx| {
                if this.chooser.as_ref().is_none_or(|c| c.asking != asking) {
                    return;
                }
                if taken == 0 {
                    let answer = this.chooser_chosen(paths);
                    this.chooser_end(answer, window, cx);
                    return;
                }
                this.open_confirm(
                    words!(
                        chooser_replace_several = "{ $count ->
                            [one] Replace the file of the same name already in { $folder }?
                           *[other] Replace the { $count } files of the same names already in { $folder }?
                        }",
                        count = taken,
                        folder = folder.display().to_string(),
                        note = "Asked before another program saves several files over files already there: count: how many are there; folder: where."
                    ),
                    crate::prompts::Confirmed::Replace(paths),
                    window,
                    cx,
                );
            },
            window,
            cx,
        );
    }

    /// A new look into whether names are taken: an answer to an older one
    /// is dropped.
    fn chooser_asks(&mut self) -> u64 {
        let Some(chooser) = &mut self.chooser else {
            return 0;
        };
        chooser.asking += 1;
        chooser.asking
    }

    /// Replace? answered yes: the paths are chosen.
    pub(crate) fn chooser_replace(
        &mut self,
        paths: Vec<PathBuf>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.chooser.is_some() {
            let answer = self.chooser_chosen(paths);
            self.chooser_end(answer, window, cx);
        }
    }

    /// `open` while choosing (Enter, a double click): a file is the answer
    /// (saving, its name is the one typed, and it is saved over once asked),
    /// a folder is gone into as ever. Whether it was taken here.
    pub(crate) fn chooser_open(&mut self, window: &mut Window, cx: &mut Context<Self>) -> bool {
        let Some(chooser) = &self.chooser else {
            return false;
        };
        let (kind, field) = (chooser.asked().kind.clone(), chooser.name.clone());
        let selected = self.chooser_selected();
        let files = selected.iter().any(|(_, dir)| !dir);
        match kind {
            // A folder opens; choosing folders, the button answers.
            Kind::Open {
                directory: true, ..
            }
            | Kind::SaveFiles { .. } => false,
            Kind::Open { .. } if files => {
                self.chooser_accept(window, cx);
                true
            }
            // The file chosen is the answer at its own path, its name as its
            // bytes are, asked Replace? on it.
            Kind::Save { .. } if files => {
                let Some(path) = selected
                    .into_iter()
                    .find(|(_, dir)| !dir)
                    .map(|(path, _)| path)
                else {
                    return false;
                };
                if let Some(name) = path.file_name() {
                    let shown = name.to_string_lossy().into_owned();
                    if let Some(chooser) = &mut self.chooser {
                        chooser.given = Some(name.to_os_string());
                    }
                    if let Some(input) = field {
                        input.update(cx, |input, cx| input.set_text(shown, cx));
                    }
                }
                self.chooser_save_at(path, window, cx);
                true
            }
            Kind::Open { .. } | Kind::Save { .. } => false,
        }
    }

    /// The filter at `index` in use (`None`: all files), its list closed.
    pub(crate) fn chooser_filter(&mut self, index: Option<usize>, cx: &mut Context<Self>) {
        let Some(chooser) = &mut self.chooser else {
            return;
        };
        let index = index.filter(|at| *at < chooser.asked().filters.len());
        chooser.open = None;
        if chooser.filter != index {
            chooser.filter = index;
            chooser.admits = admits_of(chooser.asked(), index);
            let admits = chooser.admits.clone();
            chooser.admitted.insert(self.pane);
            if let Some(pane) = self.app.workspace.write().pane_mut(self.pane) {
                pane.set_admits(admits);
            }
            self.app.wake();
        }
        cx.notify();
    }

    /// Choice `id` set to `value` (a switch's `true` or `false`, an
    /// option's id), its list closed.
    pub(crate) fn chooser_choice(&mut self, id: &str, value: &str, cx: &mut Context<Self>) {
        let Some(chooser) = &mut self.chooser else {
            return;
        };
        chooser.open = None;
        let Some(at) = chooser.asked().choices.iter().position(|c| c.id == id) else {
            return;
        };
        let choice = &chooser.asked().choices[at];
        let known = if choice.is_switch() {
            matches!(value, "true" | "false")
        } else {
            choice.options.iter().any(|(option, _)| option == value)
        };
        if known {
            value.clone_into(&mut chooser.choices[at]);
        }
        cx.notify();
    }

    /// A dropdown's list opened, or closed (`None`).
    fn chooser_list(&mut self, open: Option<Dropdown>, cx: &mut Context<Self>) {
        if let Some(chooser) = &mut self.chooser {
            chooser.open = open;
            cx.notify();
        }
    }

    /// The carrying out of `ui.chooser_*`.
    pub(crate) fn run_chooser(
        &mut self,
        name: &str,
        args: &serde_json::Value,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match name {
            "chooser_accept" => self.chooser_accept(window, cx),
            "chooser_cancel" => self.chooser_cancel(window, cx),
            "chooser_filter" => {
                let index = args["index"]
                    .as_u64()
                    .and_then(|at| usize::try_from(at).ok());
                self.chooser_filter(index, cx);
            }
            "chooser_choice" => {
                let id = args["id"].as_str().unwrap_or_default().to_owned();
                let value = match &args["value"] {
                    serde_json::Value::Bool(on) => on.to_string(),
                    value => value.as_str().unwrap_or_default().to_owned(),
                };
                self.chooser_choice(&id, &value, cx);
            }
            _ => {}
        }
    }
}

/// The bar at the window's foot while it chooses.
pub(crate) fn render(
    shell: &Shell,
    window: &Window,
    cx: &mut Context<Shell>,
) -> Option<AnyElement> {
    let chooser = shell.chooser.as_ref()?;
    let theme = cx.global::<Theme>().clone();
    let asked = chooser.asked();
    let mut fields = div()
        .flex_1()
        .min_w_0()
        .flex()
        .flex_wrap()
        .items_center()
        .gap_2();
    if let Some(input) = &chooser.name {
        let focused = input.read(cx).is_focused(window);
        fields = fields.child(
            div()
                .flex_1()
                .min_w(sized(240.))
                .flex()
                .items_center()
                .gap_2()
                .child(
                    div().flex_none().whitespace_nowrap().child(
                        words!(
                            chooser_name = "Name",
                            note = "Before the name field of the bar answering another program's Save dialog."
                        )
                        .say(),
                    ),
                )
                .child(controls::field(input, None, focused, false, None, &theme)),
        );
    }
    if !asked.filters.is_empty() {
        fields = fields.child(filter_dropdown(chooser, &theme, cx));
    }
    for at in 0..asked.choices.len() {
        fields = fields.child(choice_control(chooser, at, &theme, cx));
    }
    let buttons = div()
        .flex_none()
        .flex()
        .items_center()
        .gap_2()
        .child(modal::pressed(
            controls::button(
                "chooser-cancel",
                words!(
                    chooser_cancel = "Cancel",
                    note =
                        "A button of the bar answering another program's dialog: choose nothing."
                )
                .say(),
                None,
                ButtonStyle::Plain,
                &chooser.cancel_focus,
                &theme,
            ),
            Shell::chooser_cancel,
            cx,
        ))
        .child(modal::pressed(
            controls::button(
                "chooser-accept",
                accept_label(asked).say(),
                Some("check"),
                ButtonStyle::Plain,
                &chooser.accept_focus,
                &theme,
            ),
            Shell::chooser_accept,
            cx,
        ));
    Some(
        div()
            .id("chooser-bar")
            .role(gpui::Role::Group)
            .when(shell.a11y, |d| d.aria_label(title(asked).say()))
            .key_context(keymap::CHOOSER)
            .flex_none()
            .w_full()
            .min_h(sized(44.))
            .px_3()
            .py_2()
            .flex()
            .flex_wrap()
            .items_center()
            .justify_between()
            .gap_3()
            .text_body()
            .when(theme.lines(), |d| d.border_t_1().border_color(theme.border))
            .bg(theme.window_bar())
            .child(fields)
            .child(buttons)
            .into_any_element(),
    )
}

/// The program's filters: the one in use, and the list of them and of all
/// files a press opens.
fn filter_dropdown(chooser: &Chooser, theme: &Theme, cx: &mut Context<Shell>) -> AnyElement {
    let asked = chooser.asked();
    let open = chooser.open == Some(Dropdown::Filter);
    let label = chooser
        .filter
        .and_then(|at| asked.filters.get(at))
        .map_or_else(|| all_files().say(), |filter| filter.name.clone());
    let control = controls::dropdown("chooser-filter", label, &chooser.filter_focus, theme)
        .aria_label(
            words!(
                chooser_filter = "Files shown",
                note = "What the filter dropdown of the bar answering another program's dialog is called, for a screen reader."
            )
            .say(),
        )
        .aria_expanded(open)
        .child(chooser.filter_anchor.marker())
        .on_click(cx.listener(move |this, _, _, cx| {
            this.chooser_list((!open).then_some(Dropdown::Filter), cx);
        }));
    if !open {
        return control.into_any_element();
    }
    let mut list = controls::dropdown_list("chooser-filter-list", theme).on_mouse_down_out(
        chooser
            .filter_anchor
            .outside(cx.listener(|this, _, _, cx| this.chooser_list(None, cx))),
    );
    let rows = asked
        .filters
        .iter()
        .enumerate()
        .map(|(at, filter)| (Some(at), filter.name.clone()))
        .chain(std::iter::once((None, all_files().say())));
    for (at, label) in rows {
        list = list.child(
            controls::dropdown_row(
                format!("chooser-filter-{}", at.unwrap_or(usize::MAX)),
                at == chooser.filter,
                theme,
            )
            .child(label)
            .on_click(cx.listener(move |this, _, _, cx| this.chooser_filter(at, cx))),
        );
    }
    let width = theme.sizes.scale(crate::sizes::Part::Interface).px(260.);
    control
        .child(
            chooser
                .filter_anchor
                .popup_under(LIST_PRIORITY, width, list.into_any_element()),
        )
        .into_any_element()
}

/// Where a dropdown's list is drawn among deferred draws: over the window.
const LIST_PRIORITY: usize = 5;

/// One of the program's choices: a check for a switch, else a dropdown.
fn choice_control(
    chooser: &Chooser,
    at: usize,
    theme: &Theme,
    cx: &mut Context<Shell>,
) -> AnyElement {
    let choice = &chooser.asked().choices[at];
    let value = chooser.choices[at].clone();
    let id = choice.id.clone();
    let focus = &chooser.choice_focus[at];
    if choice.is_switch() {
        let on = value == "true";
        return modal::pressed(
            controls::check(
                format!("chooser-choice-{at}"),
                choice.label.clone(),
                on,
                focus,
                theme,
            ),
            move |this, _, cx| this.chooser_choice(&id, if on { "false" } else { "true" }, cx),
            cx,
        )
        .into_any_element();
    }
    let open = chooser.open == Some(Dropdown::Choice(at));
    let label = choice
        .options
        .iter()
        .find(|(option, _)| *option == value)
        .map_or_else(String::new, |(_, label)| label.clone());
    let anchor = &chooser.choice_anchors[at];
    let control = div()
        .flex()
        .items_center()
        .gap_2()
        .child(
            div()
                .flex_none()
                .whitespace_nowrap()
                .child(choice.label.clone()),
        )
        .child(
            controls::dropdown(format!("chooser-choice-{at}"), label, focus, theme)
                .aria_label(choice.label.clone())
                .aria_expanded(open)
                .child(anchor.marker())
                .on_click(cx.listener(move |this, _, _, cx| {
                    this.chooser_list((!open).then_some(Dropdown::Choice(at)), cx);
                }))
                .when(open, |d| {
                    let mut list =
                        controls::dropdown_list(format!("chooser-choice-list-{at}"), theme)
                            .on_mouse_down_out(anchor.outside(
                                cx.listener(|this, _, _, cx| this.chooser_list(None, cx)),
                            ));
                    for (row, (option, label)) in choice.options.iter().enumerate() {
                        let (id, option) = (id.clone(), option.clone());
                        list = list.child(
                            controls::dropdown_row(
                                format!("chooser-choice-{at}-{row}"),
                                option == value,
                                theme,
                            )
                            .child(label.clone())
                            .on_click(cx.listener(
                                move |this, _, _, cx| {
                                    this.chooser_choice(&id, &option, cx);
                                },
                            )),
                        );
                    }
                    let width = theme.sizes.scale(crate::sizes::Part::Interface).px(220.);
                    d.child(anchor.popup_under(LIST_PRIORITY, width, list.into_any_element()))
                }),
        );
    control.into_any_element()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A name's stem is what is typed over: all of a name without a
    /// suffix, or a dot file's.
    #[test]
    fn a_name_is_typed_over_up_to_its_suffix() {
        assert_eq!(stem_len("Lettre d'été.odt"), "Lettre d'été".len());
        assert_eq!(stem_len("archive.tar.gz"), "archive.tar".len());
        assert_eq!(stem_len(".bashrc"), ".bashrc".len());
        assert_eq!(stem_len("Untitled"), "Untitled".len());
    }

    /// The name saved under is the field's text as typed, its spaces kept,
    /// or, while the text still says them, the bytes of the name it was
    /// given (a name not UTF-8, shown in its lossy form): the path
    /// answered is that file's, never another's said alike.
    #[test]
    fn a_name_is_saved_under_as_its_bytes() {
        let folder = Path::new("/srv/Field Guide");
        assert_eq!(save_name("", None), None);
        assert_eq!(save_name("   ", None), None);
        assert_eq!(
            save_name(" plan .txt", None).map(|n| folder.join(n)),
            Some(folder.join(" plan .txt"))
        );
        assert_eq!(
            save_name("it's #1 %20.txt", Some(OsStr::new("other.txt"))),
            Some(OsString::from("it's #1 %20.txt")),
            "typed over, the text"
        );
        #[cfg(unix)]
        {
            use std::os::unix::ffi::OsStringExt;
            let latin = OsString::from_vec(b"r\xe9sum\xe9.txt".to_vec());
            let shown = latin.to_string_lossy().into_owned();
            assert_eq!(
                save_name(&shown, Some(&latin)).map(|n| folder.join(n)),
                Some(folder.join(&latin)),
                "the file's own bytes"
            );
            assert_eq!(
                save_name("r\u{fffd}sum\u{fffd}2.txt", Some(&latin)),
                Some(OsString::from("r\u{fffd}sum\u{fffd}2.txt")),
                "another name typed"
            );
        }
    }

    /// The asking window is read as the portal writes it; anything else
    /// names none.
    #[test]
    fn the_asking_window_is_read() {
        assert_eq!(
            foreign_parent("x11:3a0000d"),
            Some(gpui::ForeignParent::X11(0x3a0_000d))
        );
        assert_eq!(
            foreign_parent("x11:0x2c"),
            Some(gpui::ForeignParent::X11(0x2c))
        );
        assert_eq!(
            foreign_parent("wayland:6c1f5a2e-handle"),
            Some(gpui::ForeignParent::Wayland("6c1f5a2e-handle".into()))
        );
        for none in [
            "",
            "x11:",
            "x11:zz",
            "x11:0",
            "x11:0x0x2c",
            "wayland:",
            "win32:12",
        ] {
            assert_eq!(foreign_parent(none), None, "{none}");
        }
        // A handle is a short token of printable ASCII: one longer, or
        // holding anything else, names none (it would go whole into a
        // Wayland request).
        let longest = "h".repeat(HANDLE_MAX);
        assert_eq!(
            foreign_parent(&format!("wayland:{longest}")),
            Some(gpui::ForeignParent::Wayland(longest.clone()))
        );
        for none in [
            format!("wayland:{longest}h"),
            format!("wayland:{}", "h".repeat(70_000)),
            "wayland:a handle".to_owned(),
            "wayland:poign\u{e9}e".to_owned(),
            "wayland:a\nb".to_owned(),
            "wayland:a\0b".to_owned(),
        ] {
            assert_eq!(foreign_parent(&none), None, "{} bytes", none.len());
        }
    }

    /// A program's label keeps its words, not its mnemonic's mark.
    #[test]
    fn a_program_names_its_answer() {
        let mut asked = Asked {
            app_id: String::new(),
            parent_window: String::new(),
            title: String::new(),
            accept_label: Some("_Open".into()),
            kind: Kind::Open {
                multiple: false,
                directory: false,
            },
            filters: Vec::new(),
            current_filter: None,
            choices: Vec::new(),
            folder: None,
        };
        assert_eq!(accept_label(&asked).say(), "Open");
        asked.accept_label = Some("Save as __draft__".into());
        assert_eq!(accept_label(&asked).say(), "Save as _draft_");
        asked.title = "Open a picture".into();
        assert_eq!(title(&asked).say(), "Open a picture");
    }
}
