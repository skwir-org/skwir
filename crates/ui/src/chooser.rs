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

/// The folders each program last chose in (ADR-141, 7; ADR-144, 8), by
/// its id, the host's programs (no id) under "": a table each, its
/// `folders` newest first and when it last chose (`used`, seconds since
/// 1970). A folder is written as a path is, losslessly: its text, or its
/// bytes where they are not UTF-8 (`os_path::PathArg`). A program before
/// was given its one folder as a string.
pub(crate) const FOLDERS_KEY: &str = "ui.chooser_folders";

/// The folders kept for a program, those it chose in last (ADR-144, 8):
/// more than are shown, so that one gone or shown elsewhere in the
/// sidebar leaves room for the next.
const FOLDERS_KEPT: usize = 10;

/// The folders of a program's the sidebar's Recent Folders shows at most
/// (`Shell::places_here`).
pub(crate) const FOLDERS_SHOWN: usize = 5;

/// How long the folders a program chose in are looked for as its dialog
/// comes (`gone_of`); one not answered by then counts as there.
const FOLDERS_READ_WITHIN: std::time::Duration = std::time::Duration::from_millis(500);

/// The command that forgets where a program chose: a folder, or all.
pub(crate) const FORGET_FOLDERS: &str = "ui.chooser_forget_folders";

/// The programs whose folders are kept, those that chose last.
const PROGRAMS_KEPT: usize = 50;

/// Counts the times the record of where programs chose (`FOLDERS_KEY`) was
/// written in this program (`keep_folders`, its one writer): every dialog
/// reads its program's folders again once it moved (`Followed::follow`).
/// The record is what the window keeps, which moves no revision of the
/// settings.
static FOLDERS_WRITTEN: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

/// Keep `kept` as the record of where programs chose (`FOLDERS_KEY`):
/// every dialog's window follows it (`Followed`), a folder taken out or
/// chosen in through one dialog so in all of a program's (ADR-144, 8).
fn keep_folders(app: &skwir_core::App, kept: toml::Table) {
    app.settings.set(FOLDERS_KEY, toml::Value::Table(kept));
    FOLDERS_WRITTEN.fetch_add(1, std::sync::atomic::Ordering::AcqRel);
}

/// The folders a dialog shows of those its program last chose in, newest
/// first (ADR-144, 8): the record's, but those found gone as its request
/// came (`Asking::gone`), read again whenever the record was written since
/// (`FOLDERS_WRITTEN`), by this dialog or another: the sidebar's Recent
/// Folders (`Chooser::folder_places`).
struct Followed {
    folders: Vec<PathBuf>,
    /// Those found gone as the request came: kept in the record, which
    /// forgets them as the program chooses elsewhere, and never shown.
    gone: HashSet<PathBuf>,
    /// `FOLDERS_WRITTEN` as `folders` were read.
    read: u64,
    /// Counts the changes of `folders`, which the sidebar's places keep
    /// up with.
    revision: u64,
}

impl Followed {
    /// Program `program`'s folders as the record has them now.
    fn new(app: &skwir_core::App, program: &str, gone: HashSet<PathBuf>) -> Self {
        let mut followed = Followed {
            folders: Vec::new(),
            gone,
            read: 0,
            revision: 0,
        };
        followed.read_again(app, program);
        followed
    }

    /// Read program `program`'s folders again where the record was written
    /// since they were read: whether what is shown changed. Nothing is
    /// read where nothing was written.
    fn follow(&mut self, app: &skwir_core::App, program: &str) -> bool {
        self.read != FOLDERS_WRITTEN.load(std::sync::atomic::Ordering::Acquire)
            && self.read_again(app, program)
    }

    fn read_again(&mut self, app: &skwir_core::App, program: &str) -> bool {
        // Counted before the record is read: a write meanwhile reads again.
        self.read = FOLDERS_WRITTEN.load(std::sync::atomic::Ordering::Acquire);
        let folders: Vec<PathBuf> = app
            .settings
            .get::<toml::Table>(FOLDERS_KEY)
            .map(|kept| folders_of(&kept, program))
            .unwrap_or_default()
            .into_iter()
            .filter(|folder| !self.gone.contains(folder))
            .collect();
        if folders == self.folders {
            return false;
        }
        self.folders = folders;
        self.revision += 1;
        true
    }
}

/// The folders program `app` last chose in, newest first, as `kept` (the
/// value of `FOLDERS_KEY`) holds them: only folders' paths (absolute).
pub(crate) fn folders_of(kept: &toml::Table, app: &str) -> Vec<PathBuf> {
    let folders: Vec<toml::Value> = match kept.get(app) {
        Some(one @ toml::Value::String(_)) => vec![one.clone()],
        Some(toml::Value::Table(program)) => program
            .get("folders")
            .and_then(toml::Value::as_array)
            .cloned()
            .unwrap_or_default(),
        _ => Vec::new(),
    };
    folders
        .into_iter()
        .filter_map(|folder| folder.try_into::<skwir_core::os_path::PathArg>().ok())
        .map(skwir_core::os_path::PathArg::into_inner)
        .filter(|folder| folder.is_absolute())
        .collect()
}

/// `kept` with `folder` the newest of program `app`'s, chosen at `now`:
/// ten folders a program at most (`FOLDERS_KEPT`), the fifty programs that
/// chose last.
fn with_folder(mut kept: toml::Table, app: &str, folder: &Path, now: i64) -> toml::Table {
    let written =
        |folder: &Path| toml::Value::try_from(skwir_core::os_path::PathArg::from(folder)).ok();
    let folders: Vec<toml::Value> = std::iter::once(folder.to_path_buf())
        .chain(
            folders_of(&kept, app)
                .into_iter()
                .filter(|kept| kept != folder)
                .take(FOLDERS_KEPT - 1),
        )
        .filter_map(|folder| written(&folder))
        .collect();
    let mut program = toml::Table::new();
    program.insert("folders".into(), toml::Value::Array(folders));
    program.insert("used".into(), toml::Value::Integer(now));
    kept.insert(app.to_owned(), toml::Value::Table(program));
    if kept.len() > PROGRAMS_KEPT {
        let used = |value: &toml::Value| {
            value
                .get("used")
                .and_then(toml::Value::as_integer)
                .unwrap_or(0)
        };
        let mut programs: Vec<(String, i64)> = kept
            .iter()
            .map(|(app, value)| (app.clone(), used(value)))
            .collect();
        programs.sort_by_key(|(_, used)| std::cmp::Reverse(*used));
        for (app, _) in programs.into_iter().skip(PROGRAMS_KEPT) {
            kept.remove(&app);
        }
    }
    kept
}

/// What `ui.chooser_forget_folders` asks of a program's folders.
pub(crate) enum FoldersAsked {
    /// One forgotten, or all (`None`).
    Forget(Option<PathBuf>),
    /// Those it had put back, newest first, and when it last chose: what
    /// the Undo of `Forget(forgot)` runs, whose Redo asks it again.
    PutBack {
        folders: Vec<PathBuf>,
        used: Option<i64>,
        forgot: Option<PathBuf>,
    },
}

/// `kept` with program `app`'s folders `folders` again, newest first, and
/// when it last chose (`used`); none, the program forgotten.
fn put_back(
    mut kept: toml::Table,
    app: &str,
    folders: &[PathBuf],
    used: Option<i64>,
) -> toml::Table {
    let written: Vec<toml::Value> = folders
        .iter()
        .filter(|folder| folder.is_absolute())
        .take(FOLDERS_KEPT)
        .filter_map(|folder| {
            toml::Value::try_from(skwir_core::os_path::PathArg::from(folder.as_path())).ok()
        })
        .collect();
    if written.is_empty() {
        kept.remove(app);
        return kept;
    }
    let mut program = toml::Table::new();
    program.insert("folders".into(), toml::Value::Array(written));
    if let Some(used) = used {
        program.insert("used".into(), toml::Value::Integer(used));
    }
    kept.insert(app.to_owned(), toml::Value::Table(program));
    kept
}

/// What `asked` did, as its message says it: what was asked, whatever is
/// left (a program's last folder taken out is that folder taken out), and
/// the Undo by what it undoes (one taken out, or all).
fn folders_told(asked: &FoldersAsked) -> Words {
    let named = |folder: &PathBuf| {
        words::Name(folder.file_name().map_or_else(
            || folder.to_string_lossy().into_owned(),
            |name| name.to_string_lossy().into_owned(),
        ))
    };
    match asked {
        FoldersAsked::Forget(None) => words!(
            chooser_folders_cleared = "Recent Folders cleared",
            note = "Said in a window answering another program's dialog once the sidebar's Recent Folders are cleared, with Undo beside."
        ),
        FoldersAsked::Forget(Some(folder)) => words!(
            chooser_folder_forgotten = "{ $folder } taken out of Recent Folders",
            folder = named(folder),
            note = "Said in a window answering another program's dialog once a folder is taken out of the sidebar's Recent Folders, with Undo beside; folder: its name."
        ),
        FoldersAsked::PutBack {
            forgot: Some(folder),
            ..
        } => words!(
            chooser_folder_put_back = "{ $folder } put back in Recent Folders",
            folder = named(folder),
            note = "Said once the Undo of a folder taken out of the sidebar's Recent Folders puts it back; folder: its name."
        ),
        FoldersAsked::PutBack { forgot: None, .. } => words!(
            chooser_folders_put_back = "Recent Folders put back",
            note = "Said once the Undo of the sidebar's Recent Folders cleared puts them back."
        ),
    }
}

/// Another program's dialog asked (ADR-141), with what is gone of the
/// folders its program chose in, read as it came (`read_then`): its
/// window opens once that is known, never showing or starting in a folder
/// gone (ADR-144, 8).
#[derive(Clone)]
pub(crate) struct Asking {
    pub(crate) choosing: Choosing,
    gone: std::sync::Arc<std::sync::OnceLock<HashSet<PathBuf>>>,
}

impl Asking {
    pub(crate) fn new(choosing: Choosing) -> Self {
        Asking {
            choosing,
            gone: std::sync::Arc::default(),
        }
    }

    /// Whether what is gone is known: the window may open.
    pub(crate) fn read(&self) -> bool {
        self.gone.get().is_some()
    }

    /// What is gone, read on a thread of its own (`gone_of`, half a second
    /// at most), then `done`.
    pub(crate) fn read_then(
        &self,
        app: std::sync::Arc<skwir_core::App>,
        done: impl FnOnce() + Send + 'static,
    ) {
        let (gone, program) = (self.gone.clone(), self.choosing.asked().app_id.clone());
        let started = std::thread::Builder::new()
            .name("skwir-chosen-folders".into())
            .spawn(move || {
                let _ = gone.set(gone_of(&app, &program));
                done();
            });
        if started.is_err() {
            let _ = self.gone.set(HashSet::new());
        }
    }

    /// The folders found gone, none while unread.
    pub(crate) fn gone(&self) -> HashSet<PathBuf> {
        self.gone.get().cloned().unwrap_or_default()
    }
}

/// Which of the folders program `program` chose in are not there: each
/// asked on a thread of its own, at most `FOLDERS_READ_WITHIN` waited for
/// (a drive asleep, a folder one has not answered for counts as there). A
/// server's, while its mount is up, is there: nothing is asked of it
/// unasked (remote.md); one whose mount is gone is looked for where it
/// was, which says it is not there.
fn gone_of(app: &skwir_core::App, program: &str) -> HashSet<PathBuf> {
    let kept = app
        .settings
        .get::<toml::Table>(FOLDERS_KEY)
        .map(|kept| folders_of(&kept, program))
        .unwrap_or_default();
    let (sent, answers) = flume::unbounded();
    let mut asked = 0;
    for folder in kept {
        if skwir_core::fs::reach::of(&folder).is_some() {
            continue;
        }
        let sent = sent.clone();
        let started = std::thread::Builder::new()
            .name("skwir-chosen-folder".into())
            .spawn(move || {
                let there = std::fs::metadata(&folder).is_ok_and(|meta| meta.is_dir());
                let _ = sent.send((folder, there));
            });
        asked += usize::from(started.is_ok());
    }
    drop(sent);
    let until = std::time::Instant::now() + FOLDERS_READ_WITHIN;
    let mut gone = HashSet::new();
    for _ in 0..asked {
        match answers.recv_deadline(until) {
            Ok((folder, false)) => {
                gone.insert(folder);
            }
            Ok(_) => {}
            Err(_) => break,
        }
    }
    gone
}

/// `kept` without `folder` among program `app`'s, or without any of them
/// (`None`): where it chose is forgotten, and its next dialog starts as
/// one that chose nowhere would (`start_location`).
fn without_folder(mut kept: toml::Table, app: &str, folder: Option<&Path>) -> toml::Table {
    let Some(folder) = folder else {
        kept.remove(app);
        return kept;
    };
    let left: Vec<toml::Value> = folders_of(&kept, app)
        .into_iter()
        .filter(|kept| kept != folder)
        .filter_map(|left| toml::Value::try_from(skwir_core::os_path::PathArg::from(left)).ok())
        .collect();
    if left.is_empty() {
        kept.remove(app);
        return kept;
    }
    let used = kept
        .get(app)
        .and_then(|program| program.get("used"))
        .cloned();
    let mut program = toml::Table::new();
    program.insert("folders".into(), toml::Value::Array(left));
    if let Some(used) = used {
        program.insert("used".into(), used);
    }
    kept.insert(app.to_owned(), toml::Value::Table(program));
    kept
}

/// Where the window starts: the request's folder, else the one its
/// program last chose in that is there (`gone`, read as the request came,
/// ADR-144, 8), else, opening files, what lists the files used lately,
/// where the desktop remembers them and it has one to give as the request
/// came (`Asked::recent`, ADR-144, 7), else home (ADR-141, 7). Nothing is
/// read here: a folder the request names that is gone says so in the
/// pane, as any.
pub(crate) fn start_location(
    app: &skwir_core::App,
    asked: &Asked,
    gone: &HashSet<PathBuf>,
) -> Location {
    // A folder the request names that is no folder's path (a relative
    // one) is passed over, as one it does not name.
    if let Some(folder) = asked.folder.as_ref().filter(|folder| folder.is_absolute()) {
        return Location::from_path(folder.clone());
    }
    let kept = app
        .settings
        .get::<toml::Table>(FOLDERS_KEY)
        .and_then(|kept| {
            folders_of(&kept, &asked.app_id)
                .into_iter()
                .find(|folder| !gone.contains(folder))
        });
    if let Some(kept) = kept {
        return Location::from_path(kept);
    }
    let opens_files = matches!(
        asked.kind,
        Kind::Open {
            directory: false,
            ..
        }
    );
    let recent = (opens_files && asked.recent)
        .then(|| {
            app.registry
                .places
                .all()
                .into_iter()
                .map(|place| place.location)
                .find(|location| app.lists_recent(location))
        })
        .flatten();
    recent.unwrap_or_else(|| {
        Location::from_path(skwir_core::os_path::home().unwrap_or_else(|| PathBuf::from("/")))
    })
}

/// Whether the request asks for pictures or films alone, by the filter it
/// starts with (ADR-144, 5): its window takes the dialogs' view for those.
pub(crate) fn asks_pictures(asked: &Asked) -> bool {
    asked
        .first_filter()
        .and_then(|at| asked.filters.get(at))
        .is_some_and(skwir_platform::chooser::Filter::asks_pictures)
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
    /// It asks for pictures or films alone (`asks_pictures`): the window
    /// remembers its view and zoom as those requests' (ADR-144, 5).
    pub(crate) pictures: bool,
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
    /// The folders its program last chose in, newest first (ADR-144, 8),
    /// as the record has them, but those gone as the request came,
    /// following what any dialog forgets or chooses in: the sidebar's
    /// Recent Folders (`folder_places`).
    folders: Followed,
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

    /// The name of the program's filter in use, as the program gave it;
    /// none for All files.
    pub(crate) fn filter_name(&self) -> Option<String> {
        self.filter
            .and_then(|at| self.asked().filters.get(at))
            .map(|filter| filter.name.clone())
    }

    /// Whether it asks for files to open (not folders, not where to save).
    pub(crate) fn opens_files(&self) -> bool {
        matches!(
            self.asked().kind,
            Kind::Open {
                directory: false,
                ..
            }
        )
    }

    /// Whether it opens files where Recent has something to give
    /// (`Asked::recent`): Recent comes first among the places (ADR-144, 7).
    pub(crate) fn offers_recent(&self) -> bool {
        self.opens_files() && self.asked().recent
    }

    /// The folders its program last chose in, as the sidebar's places
    /// (ADR-144, 8): its group of their own, under Skwir Tabs as the
    /// sidebar ships, newest first, each under its name, told apart from
    /// another of the same name by the sidebar (`sidebar::told_apart`).
    /// Each is the user's to take out (`ui.chooser_forget_folders`, Remove
    /// From Sidebar), as the group's menu takes out all. Nothing is read: a
    /// folder gone says so in the pane, as any.
    pub(crate) fn folder_places(&self) -> Vec<skwir_core::registry::Place> {
        self.folders
            .folders
            .iter()
            .map(|folder| {
                let name = folder.file_name().map_or_else(
                    || folder.to_string_lossy().into_owned(),
                    |name| name.to_string_lossy().into_owned(),
                );
                skwir_core::registry::Place {
                    forget: Some(skwir_core::command::Invocation::with_args(
                        FORGET_FOLDERS,
                        &serde_json::json!({
                            "folder": skwir_core::os_path::PathArg::from(folder.as_path()),
                        }),
                    )),
                    ..skwir_core::registry::Place::new(
                        skwir_core::registry::groups::RECENT_FOLDERS.id,
                        Words::as_written(name),
                        "folder",
                        Location::from_path(folder.clone()),
                    )
                }
            })
            .collect()
    }

    /// Counts the changes of the folders its program chose in.
    pub(crate) fn folders_revision(&self) -> u64 {
        self.folders.revision
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
/// choosing folders, no file, its files shown dimmed (ADR-144, 9). One
/// that admits every file (the program's All files) leaves the pane
/// unfiltered, as the window's own does.
fn admits_of(asked: &Asked, filter: Option<usize>) -> Option<Admits> {
    if matches!(
        asked.kind,
        Kind::Open {
            directory: true,
            ..
        }
    ) {
        return Some(Admits::folders());
    }
    let filter = asked.filters.get(filter?)?;
    (!filter.admits_all()).then(|| Admits::taking(filter.admits()))
}

/// What of `paths` is taken: how many (a file, a link, anything there),
/// and the first a folder takes (followed through a link), which no file
/// can be saved over. Reads the disk: on a worker.
fn taken(paths: &[PathBuf]) -> (usize, Option<PathBuf>) {
    let mut count = 0;
    let mut folder = None;
    for path in paths {
        let Ok(meta) = std::fs::symlink_metadata(path) else {
            continue;
        };
        count += 1;
        if folder.is_none() && (meta.is_dir() || std::fs::metadata(path).is_ok_and(|m| m.is_dir()))
        {
            folder = Some(path.clone());
        }
    }
    (count, folder)
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

/// What the answer would give now, as its button counts it (ADR-144,
/// 11): the files and the folders selected.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct Given {
    files: usize,
    folders: usize,
}

/// The answer's button: the program's words, as it gave them, else what
/// it does, saying how many where it gives several (ADR-144, 11): the
/// files or the folders it would give (`given`), the files saved.
fn accept_label(asked: &Asked, given: Given) -> Words {
    if let Some(label) = &asked.accept_label {
        // A label written with a mnemonic (GTK's `_Open`, `__` for an
        // underscore) says it plainly.
        let plain: Vec<String> = label
            .split("__")
            .map(|part| part.replace('_', ""))
            .collect();
        return Words::as_written(plain.join("_"));
    }
    match &asked.kind {
        Kind::Open {
            directory: true,
            multiple: true,
        } if given.folders > 1 => words!(
            chooser_accept_folders = "{ $count ->
                [one] Select { $count } folder
               *[other] Select { $count } folders
            }",
            count = given.folders,
            note = "A button of the bar answering another program's dialog: choose the folders selected; count: how many."
        ),
        Kind::Open {
            directory: true, ..
        } => words!(
            chooser_accept_folder = "Select",
            note = "A button of the bar answering another program's dialog: choose the folder."
        ),
        Kind::Open { multiple: true, .. } if given.files > 1 => words!(
            chooser_accept_open_files = "{ $count ->
                [one] Open { $count } file
               *[other] Open { $count } files
            }",
            count = given.files,
            note = "A button of the bar answering another program's dialog: open the files selected; count: how many."
        ),
        Kind::Open { .. } => words!(
            chooser_accept_open = "Open",
            note = "A button of the bar answering another program's dialog: open the files chosen."
        ),
        Kind::SaveFiles { files } if files.len() > 1 => words!(
            chooser_accept_save_files = "{ $count ->
                [one] Save { $count } file
               *[other] Save { $count } files
            }",
            count = files.len(),
            note = "A button of the bar answering another program's dialog: save the files it names in the folder shown; count: how many."
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
    /// filter in use. The pane shows where it starts (`start_location`).
    pub(crate) fn answer_dialog(
        &mut self,
        asking: Asking,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let gone = asking.gone();
        let choosing = asking.choosing;
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
            pictures: asks_pictures(&asked),
            admits: admits_of(&asked, filter),
            admitted: HashSet::new(),
            choosing,
            name,
            filter,
            choices,
            open: None,
            folders: Followed::new(&self.app, &asked.app_id, gone),
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
    /// on the pane shown (another tab's too, once shown), the folders its
    /// program chose in as the record has them (`Followed`), and the
    /// window gone once the program gave up.
    pub(crate) fn chooser_sync(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(chooser) = &mut self.chooser else {
            return;
        };
        if chooser.choosing.is_closed() {
            tracing::info!("the program gave up its dialog");
            self.chooser_end(Answer::Cancelled, window, cx);
            return;
        }
        let program = chooser.choosing.asked().app_id.clone();
        chooser.folders.follow(&self.app, &program);
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
            // Files chosen are the desktop's recently used, as its own
            // dialog adds them (ADR-144, 7); folders are not.
            let choosing_folders = matches!(
                chooser.asked().kind,
                Kind::Open {
                    directory: true,
                    ..
                }
            );
            // In the files' lane: each is typed by what it is, read there.
            if !choosing_folders {
                let paths = paths.clone();
                // A skwir the bus started for this dialog ends with its
                // window: its end waits for this (`finishing::Writing`).
                let writing = crate::finishing::Writing::new();
                self.app.scheduler.spawn_in(
                    crate::shell::lane_of(&paths),
                    skwir_core::scheduler::Priority::Normal,
                    "ui-chooser-recent",
                    self.app.lifetime().child(),
                    move || {
                        let _writing = writing;
                        if let Err(error) = skwir_platform::recent::add(&paths) {
                            tracing::warn!("the recently used files were not written: {error:#}");
                        }
                    },
                );
            }
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

    /// The folder its program chose in, kept first of its folders for its
    /// next dialog (ADR-141, 7; ADR-144, 8): saving, the one saved in (the
    /// answer's); opening, the one shown, else the first path's.
    fn chooser_remember(&self, asked: &Asked, paths: &[PathBuf]) {
        let answered_in = || {
            paths
                .first()
                .and_then(|p| p.parent())
                .map(Path::to_path_buf)
        };
        // Saving, where the answer saves: the folder selected for several
        // files, a folder typed with a name.
        let saves = matches!(asked.kind, Kind::Save { .. } | Kind::SaveFiles { .. });
        let folder = if saves {
            answered_in()
        } else {
            self.app.pane_dir(self.pane).ok().or_else(answered_in)
        };
        let Some(folder) = folder else {
            return;
        };
        let kept = self
            .app
            .settings
            .get::<toml::Table>(FOLDERS_KEY)
            .unwrap_or_default();
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |since| {
                i64::try_from(since.as_secs()).unwrap_or(i64::MAX)
            });
        let kept = with_folder(kept, &asked.app_id, &folder, now);
        self.chooser_keep_folders(kept);
    }

    /// `keep_folders`, saved later, and every window woken: the other
    /// dialogs of a program show its folders as kept now (`Followed`).
    fn chooser_keep_folders(&self, kept: toml::Table) {
        keep_folders(&self.app, kept);
        self.save_settings_later();
        self.app.wake();
    }

    /// Files dropped on a dialog's window, from a window of the user's own
    /// or another program (ADR-144, 10): gone to, never copied or moved,
    /// nor answered. Opening, the first's folder is shown and what of them
    /// is in it selected (the first alone where the request takes one);
    /// saving, a folder dropped is gone into, a file's folder shown and
    /// its name taken. Whether the window took the drop: any, in a
    /// dialog's. Whether the first is a folder is read on a worker, in its
    /// lane.
    pub(crate) fn dialog_takes_drop(
        &mut self,
        paths: &[PathBuf],
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        if self.chooser.is_none() {
            return false;
        }
        let Some(first) = paths.first().cloned() else {
            return true;
        };
        let paths = paths.to_vec();
        self.off_thread(
            crate::shell::lane_of(std::slice::from_ref(&first)),
            "ui-chooser-drop",
            move || std::fs::metadata(&first).is_ok_and(|meta| meta.is_dir()),
            move |shell, first_is_folder, window, cx| {
                shell.chooser_go_to_dropped(&paths, first_is_folder, window, cx);
            },
            window,
            cx,
        );
        true
    }

    /// `dialog_takes_drop`, once whether the first is a folder is known.
    fn chooser_go_to_dropped(
        &mut self,
        paths: &[PathBuf],
        first_is_folder: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(chooser) = &self.chooser else {
            return;
        };
        let Some(first) = paths.first() else {
            return;
        };
        let saving = matches!(chooser.asked().kind, Kind::Save { .. });
        let several = matches!(chooser.asked().kind, Kind::Open { multiple: true, .. });
        let field = chooser.name.clone();
        if saving && first_is_folder {
            let go = skwir_core::command::Invocation::navigate(&Location::from_path(first.clone()));
            self.run(&go.name, go.args, window, cx);
            return;
        }
        let Some(folder) = first.parent().map(Path::to_path_buf) else {
            return;
        };
        let go = skwir_core::command::Invocation::navigate(&Location::from_path(folder.clone()));
        self.run(&go.name, go.args, window, cx);
        if saving {
            if let (Some(input), Some(name)) = (field, first.file_name()) {
                let shown = name.to_string_lossy().into_owned();
                if let Some(chooser) = &mut self.chooser {
                    chooser.given = Some(name.to_os_string());
                }
                input.update(cx, |input, cx| {
                    let stem = stem_len(&shown);
                    input.set_text(shown, cx);
                    input.select_range(0..stem, cx);
                });
            }
            return;
        }
        let names: Vec<OsString> = paths
            .iter()
            .filter(|path| path.parent() == Some(folder.as_path()))
            .take(if several { usize::MAX } else { 1 })
            .filter_map(|path| path.file_name().map(OsStr::to_os_string))
            .collect();
        self.app.reveal_names(self.pane, names);
        cx.notify();
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

    /// What the answer would give, as its button counts it (`Given`): the
    /// files and the folders among the selection, from its cached summary
    /// (`selection_ctx`); none where what the pane lists is not to give,
    /// as `chooser_selected`.
    fn chooser_given(&self) -> Given {
        let gives = self
            .app
            .pane_location(self.pane)
            .is_none_or(|location| self.app.gives_files(&location));
        if !gives {
            return Given::default();
        }
        let selection = self.selection_ctx();
        Given {
            files: selection.files,
            folders: selection.dirs,
        }
    }

    /// What is selected in the pane, each with whether it is a folder, the
    /// cursor's first.
    /// Nothing, where what the pane lists is not to give (the Trash, gone
    /// to by its address, ADR-144, 3).
    fn chooser_selected(&self) -> Vec<(PathBuf, bool)> {
        let gives = self
            .app
            .pane_location(self.pane)
            .is_none_or(|location| self.app.gives_files(&location));
        if !gives {
            return Vec::new();
        }
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
                    self.chooser_nothing_given(&kind, cx);
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
                move || taken(&paths)
            },
            move |this, (taken, folder_taken), window, cx| {
                if this.chooser.as_ref().is_none_or(|c| c.asking != asking) {
                    return;
                }
                // No file can be saved over a folder: said, and nothing
                // answered, another folder to be chosen.
                if let Some(taken) = folder_taken {
                    this.set_status(
                        words!(
                            chooser_folder_taken = "There is a folder named { $name } in { $folder }: choose another folder to save in",
                            name = skwir_core::words::Name::of(&taken),
                            folder = folder.as_path(),
                            note = "Said where another program saves several files in a folder holding a folder of the name of one of them, which no file can replace: name: that folder's name; folder: where it is."
                        ),
                        true,
                        cx,
                    );
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

    /// `open` while choosing, the one answer `open` has in a window
    /// answering another program's dialog (ADR-141, 5; ADR-144, 1): what it
    /// names (`entries`: the address field's Shift+Enter, a file used
    /// lately), read on a worker in their lane, else the entry under the
    /// cursor and the rest selected (Enter, a double click,
    /// `chooser_selected`). Never a file opened in another program: whether
    /// the window answers a dialog, where it takes every `open`.
    pub(crate) fn chooser_open(
        &mut self,
        args: &serde_json::Value,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        if self.chooser.is_none() {
            return false;
        }
        let named = named(args);
        if named.is_empty() {
            let chosen = self.chooser_selected();
            self.chooser_open_these(chosen, window, cx);
            return true;
        }
        let asking = self.chooser_asks();
        self.off_thread(
            crate::shell::lane_of(&named),
            "ui-chooser-open",
            move || {
                named
                    .into_iter()
                    .filter_map(|path| {
                        // A link to a folder is the folder it opens.
                        let dir = std::fs::metadata(&path).ok()?.is_dir();
                        Some((path, dir))
                    })
                    .collect::<Vec<_>>()
            },
            move |this, chosen, window, cx| {
                if this.chooser.as_ref().is_some_and(|c| c.asking == asking) {
                    this.chooser_open_these(chosen, window, cx);
                }
            },
            window,
            cx,
        );
        true
    }

    /// `chooser_open` for what it found, the first deciding: a folder is
    /// gone into, alone, whatever else is chosen; a file is the answer
    /// (saving, its name is the one typed, and it is saved over once
    /// asked), or, choosing folders or where to save several, taken and
    /// never opened in another program from here: it only says where one
    /// is (ADR-144, 9). Nothing to give is said (`chooser_nothing_given`).
    fn chooser_open_these(
        &mut self,
        chosen: Vec<(PathBuf, bool)>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(chooser) = &self.chooser else {
            return;
        };
        let (kind, field) = (chooser.asked().kind.clone(), chooser.name.clone());
        let Some((path, dir)) = chosen.first().cloned() else {
            self.chooser_nothing_given(&kind, cx);
            return;
        };
        if dir {
            let go = skwir_core::command::Invocation::navigate(&Location::from_path(path));
            self.run(&go.name, go.args, window, cx);
            return;
        }
        match kind {
            Kind::Open {
                directory: true, ..
            }
            | Kind::SaveFiles { .. } => {}
            Kind::Open {
                multiple,
                directory: false,
            } => {
                let files: Vec<PathBuf> = chosen
                    .into_iter()
                    .filter(|(_, dir)| !dir)
                    .map(|(path, _)| path)
                    .take(if multiple { usize::MAX } else { 1 })
                    .collect();
                let answer = self.chooser_chosen(files);
                self.chooser_end(answer, window, cx);
            }
            // The file chosen is the answer at its own path, its name as its
            // bytes are, asked Replace? on it.
            Kind::Save { .. } => {
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
            }
        }
    }

    /// What is said where `open` or the answer's button found nothing to
    /// give: the pane shows a place whose files are not to give (the Trash,
    /// typed in the address, ADR-144, 3), or, choosing files, none is
    /// selected.
    fn chooser_nothing_given(&mut self, kind: &Kind, cx: &mut Context<Self>) {
        let place = self
            .app
            .pane_location(self.pane)
            .filter(|location| !self.app.gives_files(location));
        if let Some(place) = place {
            let place = self.app.location_title(&place);
            self.set_status(
                words!(
                    chooser_not_given = "Files in { $place } cannot be chosen",
                    place = place,
                    note = "Said where another program's dialog was asked to answer with a file of a place whose files are not where they were (the Trash). place: its name (Trash)."
                ),
                true,
                cx,
            );
        } else if matches!(
            kind,
            Kind::Open {
                directory: false,
                ..
            }
        ) {
            self.set_status(choose_a_file_first(), true, cx);
        }
    }

    /// The filter at `index` in use (`None`: all files, the program's own
    /// where it sent one, `Asked::all_files`), its list closed.
    pub(crate) fn chooser_filter(&mut self, index: Option<usize>, cx: &mut Context<Self>) {
        let Some(chooser) = &mut self.chooser else {
            return;
        };
        let index = index
            .filter(|at| *at < chooser.asked().filters.len())
            .or_else(|| chooser.asked().all_files());
        chooser.open = None;
        if chooser.filter != index {
            // Saving, the name takes the new filter's extension in place of
            // one the program's filters take (ADR-144, 6): one typed that
            // none takes stays, as one the new filter takes. Swapped on the
            // name's bytes while the field still says them.
            let filters = &chooser.asked().filters;
            let swap = chooser.name.clone().and_then(|input| {
                let to = filters.get(index?)?;
                let name = save_name(input.read(cx).text(), chooser.given.as_deref())?;
                let swapped = skwir_platform::chooser::swapped_name(&name, filters, to)?;
                Some((input, swapped))
            });
            chooser.filter = index;
            chooser.admits = admits_of(chooser.asked(), index);
            let admits = chooser.admits.clone();
            chooser.admitted.insert(self.pane);
            if let Some(pane) = self.app.workspace.write().pane_mut(self.pane) {
                pane.set_admits(admits);
            }
            if let Some((input, swapped)) = swap {
                let shown = swapped.to_string_lossy().into_owned();
                chooser.given = Some(swapped);
                input.update(cx, |input, cx| {
                    let stem = stem_len(&shown);
                    input.set_text(shown, cx);
                    input.select_range(stem..stem, cx);
                });
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

    /// Forget that its program chose in a folder, or anywhere, or put
    /// back what was forgotten (`FoldersAsked`): taken out of the sidebar's
    /// Recent Folders, and of where its next dialog may start (ADR-144, 8),
    /// in every dialog of the program (`Followed`). Said as it was asked,
    /// with its Undo (`undo`: this is one, said with its Redo, which asks
    /// again what it undid), as an edit of bookmarks is; nothing said where
    /// nothing changed (another dialog of the program forgot it first: this
    /// one shows the record as it is).
    pub(crate) fn chooser_forget_folders(
        &mut self,
        asked: FoldersAsked,
        undo: bool,
        cx: &mut Context<Self>,
    ) {
        let Some(chooser) = &mut self.chooser else {
            return;
        };
        let app_id = chooser.asked().app_id.clone();
        let kept = self
            .app
            .settings
            .get::<toml::Table>(FOLDERS_KEY)
            .unwrap_or_default();
        let before = folders_of(&kept, &app_id);
        let used = kept
            .get(&app_id)
            .and_then(|program| program.get("used"))
            .and_then(toml::Value::as_integer);
        let (kept, inverse) = match &asked {
            FoldersAsked::Forget(folder) => {
                let restore: Vec<skwir_core::os_path::PathArg> = before
                    .iter()
                    .map(|folder| skwir_core::os_path::PathArg::from(folder.as_path()))
                    .collect();
                let forgot = folder.as_deref().map(skwir_core::os_path::PathArg::from);
                (
                    without_folder(kept, &app_id, folder.as_deref()),
                    serde_json::json!({
                        "restore": restore, "used": used, "folder": forgot, "undo": !undo,
                    }),
                )
            }
            FoldersAsked::PutBack {
                folders,
                used,
                forgot,
            } => {
                let forgot = forgot.as_deref().map(skwir_core::os_path::PathArg::from);
                (
                    put_back(kept, &app_id, folders, *used),
                    serde_json::json!({ "folder": forgot, "undo": !undo }),
                )
            }
        };
        let after = folders_of(&kept, &app_id);
        if after == before {
            chooser.folders.follow(&self.app, &app_id);
            cx.notify();
            return;
        }
        self.chooser_keep_folders(kept);
        if let Some(chooser) = &mut self.chooser {
            chooser.folders.follow(&self.app, &app_id);
        }
        self.app.notify_undoable(
            folders_told(&asked),
            undo,
            skwir_core::command::Invocation::with_args(FORGET_FOLDERS, &inverse),
        );
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
            "chooser_forget_folders" => {
                let path = |value: &serde_json::Value| {
                    serde_json::from_value::<skwir_core::os_path::PathArg>(value.clone())
                        .ok()
                        .map(skwir_core::os_path::PathArg::into_inner)
                };
                let folder = args
                    .get("folder")
                    .filter(|folder| !folder.is_null())
                    .and_then(path);
                let asked = match args.get("restore").and_then(|r| r.as_array()) {
                    Some(folders) => FoldersAsked::PutBack {
                        folders: folders.iter().filter_map(path).collect(),
                        used: args["used"].as_i64(),
                        forgot: folder,
                    },
                    None => FoldersAsked::Forget(folder),
                };
                let undo = args["undo"].as_bool().unwrap_or(false);
                self.chooser_forget_folders(asked, undo, cx);
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
                accept_label(asked, shell.chooser_given()).say(),
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
            .key_context({
                // A list open says so: Escape closes it before the dialog.
                let mut context = gpui::KeyContext::default();
                context.add(keymap::CHOOSER);
                if chooser.open.is_some() {
                    context.add("lists");
                }
                context
            })
            .on_action(cx.listener(|this, action: &keymap::RunCommand, _, cx| {
                if action.name == "ui.toolbar_close"
                    && this.chooser.as_ref().is_some_and(|c| c.open.is_some())
                {
                    this.chooser_list(None, cx);
                } else {
                    cx.propagate();
                }
            }))
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
/// files a press opens, the program's own All files standing for it where
/// it sent one (`Asked::all_files`): never two rows for every file.
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
        .chain(
            asked
                .all_files()
                .is_none()
                .then(|| (None, all_files().say())),
        );
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

/// The files `open` names (`entries`), in their order: absolute paths
/// alone, as the window names what it shows.
fn named(args: &serde_json::Value) -> Vec<PathBuf> {
    args.get("entries")
        .and_then(|entries| {
            serde_json::from_value::<Vec<skwir_core::op::EntryArg>>(entries.clone()).ok()
        })
        .unwrap_or_default()
        .into_iter()
        .map(|entry| match entry {
            skwir_core::op::EntryArg::Path(path) | skwir_core::op::EntryArg::Named { path, .. } => {
                path.into_inner()
            }
        })
        .filter(|path| path.is_absolute())
        .collect()
}

/// "Choose a file first": said where another program's Open dialog was
/// asked to answer with no file chosen.
fn choose_a_file_first() -> Words {
    words!(
        chooser_choose_file = "Choose a file first",
        note = "Said where the bar answering another program's Open dialog was pressed with no file selected."
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    /// What `open` names in a dialog is what it answers with (ADR-144,
    /// 1): a path, or a file named by its identity and path, in their
    /// order; nothing for a relative path or for none named, when the
    /// selection answers.
    #[test]
    fn open_names_what_it_answers_with() {
        let args = serde_json::json!({ "entries": [
            "/home/u/one's file.txt",
            { "id": { "dev": 1, "ino": 2 }, "path": "/home/u/tröis.png" },
            "relative.txt",
        ], "pane": 3 });
        assert_eq!(
            named(&args),
            [
                PathBuf::from("/home/u/one's file.txt"),
                PathBuf::from("/home/u/tröis.png")
            ]
        );
        assert!(named(&serde_json::json!({ "pane": 3 })).is_empty());
    }

    /// A name's stem is what is typed over: all of a name without a
    /// suffix, or a dot file's.
    #[test]
    fn a_name_is_typed_over_up_to_its_suffix() {
        assert_eq!(stem_len("Lettre d'été.odt"), "Lettre d'été".len());
        assert_eq!(stem_len("archive.tar.gz"), "archive.tar".len());
        assert_eq!(stem_len(".bashrc"), ".bashrc".len());
        assert_eq!(stem_len("Untitled"), "Untitled".len());
    }

    /// A program keeps its ten last folders, newest first, the one
    /// before read as its list; the fifty programs that chose last are
    /// kept (ADR-144, 8).
    #[test]
    fn a_program_keeps_its_last_folders() {
        let mut kept = toml::Table::new();
        kept.insert(
            "org.example.Old".into(),
            toml::Value::String("/home/a/Old".into()),
        );
        kept.insert(
            "relative".into(),
            toml::Value::String("not/a/folder".into()),
        );
        assert_eq!(
            folders_of(&kept, "org.example.Old"),
            [PathBuf::from("/home/a/Old")]
        );
        assert!(folders_of(&kept, "relative").is_empty());
        assert!(folders_of(&kept, "org.example.None").is_empty());
        let kept = with_folder(kept, "org.example.Old", Path::new("/home/a/New"), 10);
        assert_eq!(
            folders_of(&kept, "org.example.Old"),
            [PathBuf::from("/home/a/New"), PathBuf::from("/home/a/Old")]
        );
        let mut kept = kept;
        for at in 0..12 {
            kept = with_folder(kept, "", Path::new(&format!("/home/a/{at}")), 20 + at);
        }
        kept = with_folder(kept, "", Path::new("/home/a/3"), 40);
        let host: Vec<String> = folders_of(&kept, "")
            .iter()
            .map(|f| f.display().to_string())
            .collect();
        assert_eq!(
            host,
            [
                "/home/a/3",
                "/home/a/11",
                "/home/a/10",
                "/home/a/9",
                "/home/a/8",
                "/home/a/7",
                "/home/a/6",
                "/home/a/5",
                "/home/a/4",
                "/home/a/2"
            ],
            "ten kept, the newest first"
        );
        for at in 0..PROGRAMS_KEPT {
            kept = with_folder(
                kept,
                &format!("org.example.P{at}"),
                Path::new("/home/a"),
                100 + i64::try_from(at).unwrap(),
            );
        }
        assert_eq!(kept.len(), PROGRAMS_KEPT);
        assert!(!kept.contains_key("org.example.Old"), "the oldest goes");
        assert!(kept.contains_key("org.example.P0"));
        // A folder whose name is not UTF-8 is kept as its bytes, and read
        // back as it was (CH-15).
        #[cfg(unix)]
        {
            use std::os::unix::ffi::OsStringExt;
            let latin = PathBuf::from(std::ffi::OsString::from_vec(b"/srv/caf\xe9".to_vec()));
            let kept = with_folder(kept, "org.example.P0", &latin, 200);
            assert_eq!(
                folders_of(&kept, "org.example.P0"),
                [latin, PathBuf::from("/home/a")]
            );
            let text = toml::to_string(&kept).unwrap();
            let again: toml::Table = toml::from_str(&text).unwrap();
            assert_eq!(
                folders_of(&again, "org.example.P0")[0],
                folders_of(&kept, "org.example.P0")[0]
            );
        }
    }

    /// An Undo puts a program's folders back as they were, with when it
    /// last chose; and what each change did is said as it was asked: one
    /// taken out (its last too), all, and the Undo of either (RD-8).
    #[test]
    fn forgotten_folders_are_put_back_and_said() {
        let (one, two) = (PathBuf::from("/home/a/one"), PathBuf::from("/home/a/two"));
        let kept = put_back(
            toml::Table::new(),
            "org.example.App",
            &[one.clone(), two.clone()],
            Some(7),
        );
        assert_eq!(
            folders_of(&kept, "org.example.App"),
            [one.clone(), two.clone()]
        );
        assert_eq!(
            kept["org.example.App"].get("used"),
            Some(&toml::Value::Integer(7))
        );
        let none = put_back(kept, "org.example.App", &[], None);
        assert!(!none.contains_key("org.example.App"));
        let said = |asked: FoldersAsked| folders_told(&asked).english();
        assert_eq!(said(FoldersAsked::Forget(None)), "Recent Folders cleared");
        // Its last folder taken out is that folder taken out, not a clear.
        assert_eq!(
            said(FoldersAsked::Forget(Some(one.clone()))),
            "“one” taken out of Recent Folders"
        );
        let undone = |forgot: Option<PathBuf>| FoldersAsked::PutBack {
            folders: vec![one.clone(), two.clone()],
            used: None,
            forgot,
        };
        assert_eq!(
            said(undone(Some(two.clone()))),
            "“two” put back in Recent Folders"
        );
        assert_eq!(said(undone(None)), "Recent Folders put back");
    }

    /// Every dialog of a program follows where it chose as the record
    /// has it (RS-4): a folder taken out through one dialog is gone from
    /// another's Recent Folders, which reads the record again only once it
    /// was written; a folder found gone as its request came stays out.
    #[test]
    fn every_dialog_of_a_program_follows_its_folders() {
        let tmp = tempfile::tempdir().unwrap();
        let app = skwir_core::App::new(skwir_core::app::AppConfig::ephemeral(tmp.path())).unwrap();
        let program = "org.example.App";
        let (one, two, gone) = (
            PathBuf::from("/home/a/one"),
            PathBuf::from("/home/a/two"),
            PathBuf::from("/home/a/gone"),
        );
        let kept = put_back(
            toml::Table::new(),
            program,
            &[one.clone(), two.clone(), gone.clone()],
            None,
        );
        keep_folders(&app, kept.clone());
        let mut first = Followed::new(&app, program, HashSet::new());
        let mut second = Followed::new(&app, program, HashSet::from([gone.clone()]));
        assert_eq!(first.folders, [one.clone(), two.clone(), gone.clone()]);
        assert_eq!(second.folders, [one.clone(), two.clone()]);
        assert!(!second.follow(&app, program), "nothing written since");
        keep_folders(&app, without_folder(kept, program, Some(&one)));
        let revision = second.revision;
        assert!(second.follow(&app, program));
        assert_eq!(second.folders, std::slice::from_ref(&two));
        assert!(second.revision > revision, "the sidebar arranges again");
        assert!(first.follow(&app, program));
        assert_eq!(first.folders, [two, gone]);
        assert!(!first.follow(&app, program));
    }

    /// A folder gone is told from one there, a server's never asked; the
    /// start passes over a folder gone for the next there.
    #[test]
    fn a_folder_gone_is_passed_over() {
        let tmp = tempfile::tempdir().unwrap();
        let app = skwir_core::App::new(skwir_core::app::AppConfig::ephemeral(tmp.path())).unwrap();
        let (there, gone) = (tmp.path().join("there"), tmp.path().join("gone"));
        std::fs::create_dir(&there).unwrap();
        let kept = put_back(
            toml::Table::new(),
            "org.example.App",
            &[gone.clone(), there.clone()],
            None,
        );
        app.settings.set(FOLDERS_KEY, toml::Value::Table(kept));
        let found = gone_of(&app, "org.example.App");
        assert_eq!(found, HashSet::from([gone]));
        let asked = Asked {
            app_id: "org.example.App".into(),
            parent_window: String::new(),
            title: String::new(),
            accept_label: None,
            kind: Kind::Open {
                multiple: false,
                directory: false,
            },
            filters: Vec::new(),
            current_filter: None,
            choices: Vec::new(),
            folder: None,
            recent: false,
        };
        assert_eq!(
            start_location(&app, &asked, &found),
            Location::from_path(there)
        );
    }

    /// A folder a program chose in is forgotten alone, the others kept in
    /// their order and when it last chose; all of them at once, or its
    /// last, and the program is forgotten; another program's stay.
    #[test]
    fn a_program_forgets_where_it_chose() {
        let mut kept = toml::Table::new();
        for (at, folder) in ["/home/a/one", "/home/a/two", "/home/a/three"]
            .iter()
            .enumerate()
        {
            kept = with_folder(kept, "org.example.App", Path::new(folder), 10 + at as i64);
        }
        kept = with_folder(kept, "org.example.Other", Path::new("/srv"), 5);
        let kept = without_folder(kept, "org.example.App", Some(Path::new("/home/a/two")));
        assert_eq!(
            folders_of(&kept, "org.example.App"),
            [PathBuf::from("/home/a/three"), PathBuf::from("/home/a/one")]
        );
        assert_eq!(
            kept["org.example.App"].get("used"),
            Some(&toml::Value::Integer(12))
        );
        let unknown = without_folder(kept.clone(), "org.example.App", Some(Path::new("/x")));
        assert_eq!(unknown, kept, "a folder it never chose in");
        let all = without_folder(kept.clone(), "org.example.App", None);
        assert!(!all.contains_key("org.example.App"));
        assert_eq!(
            folders_of(&all, "org.example.Other"),
            [PathBuf::from("/srv")]
        );
        let one = without_folder(kept, "org.example.App", Some(Path::new("/home/a/one")));
        let last = without_folder(one, "org.example.App", Some(Path::new("/home/a/three")));
        assert!(!last.contains_key("org.example.App"), "its last forgotten");
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

    /// What is taken is counted, and a folder of a name, followed through
    /// a link, is told: no file is saved over it.
    #[test]
    fn a_folder_of_a_name_is_told() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        std::fs::write(root.join("notes.txt"), "a").unwrap();
        std::fs::create_dir(root.join("taken.txt")).unwrap();
        let names =
            |names: &[&str]| -> Vec<PathBuf> { names.iter().map(|n| root.join(n)).collect() };
        assert_eq!(taken(&names(&["new.txt", "other.txt"])), (0, None));
        assert_eq!(taken(&names(&["notes.txt", "new.txt"])), (1, None));
        assert_eq!(
            taken(&names(&["notes.txt", "taken.txt"])),
            (2, Some(root.join("taken.txt")))
        );
        #[cfg(unix)]
        {
            std::os::unix::fs::symlink(root.join("taken.txt"), root.join("link.txt")).unwrap();
            assert_eq!(
                taken(&names(&["link.txt"])),
                (1, Some(root.join("link.txt"))),
                "a link to a folder is a folder"
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
            recent: false,
        };
        let none = Given::default();
        assert_eq!(accept_label(&asked, none).say(), "Open");
        asked.accept_label = Some("Save as __draft__".into());
        assert_eq!(accept_label(&asked, none).say(), "Save as _draft_");
        asked.title = "Open a picture".into();
        assert_eq!(title(&asked).say(), "Open a picture");

        // Skwir's own words count what is given (ADR-144, 11); the
        // program's are said as given.
        // Three files and two folders selected: what is counted is what
        // the answer gives, the files or the folders.
        let files = Given {
            files: 3,
            folders: 0,
        };
        let folders = Given {
            files: 0,
            folders: 2,
        };
        let both = Given {
            files: 3,
            folders: 2,
        };
        assert_eq!(accept_label(&asked, files).say(), "Save as _draft_");
        asked.accept_label = None;
        assert_eq!(accept_label(&asked, files).say(), "Open", "one taken");
        asked.kind = Kind::Open {
            multiple: true,
            directory: false,
        };
        assert_eq!(accept_label(&asked, files).say(), "Open 3 files");
        assert_eq!(accept_label(&asked, folders).say(), "Open", "no files");
        assert_eq!(
            accept_label(&asked, both).say(),
            "Open 3 files",
            "the files alone, which it gives"
        );
        asked.kind = Kind::Open {
            multiple: true,
            directory: true,
        };
        assert_eq!(accept_label(&asked, folders).say(), "Select 2 folders");
        assert_eq!(
            accept_label(&asked, both).say(),
            "Select 2 folders",
            "the folders alone, the dimmed files selected too"
        );
        assert_eq!(accept_label(&asked, none).say(), "Select", "in the Trash");
        asked.kind = Kind::SaveFiles {
            files: vec!["a.txt".into(), "b.txt".into()],
        };
        assert_eq!(accept_label(&asked, none).say(), "Save 2 files");
    }
}
