//! What becomes of the names a plan found taken where it copies, moves,
//! links or makes something (ADR-054, ADR-059): one dialog lists them all,
//! each choice made for one
//! item or for all of a kind, answered before anything is done. It waits
//! in the review queue (`review.rs`) and is answered as a review is: Enter
//! goes on, Escape cancels; the arrows walk its rows and change their
//! choice. Each row is pictured by what comes, as the views draw it, read
//! on a worker once the question arrives (`read_pictures`).

use crate::sizes::TextSizes;
use crate::sizes::sized;
use std::collections::HashMap;
use std::ffi::OsString;
use std::ops::Range;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};

use gpui::{
    AnyElement, Context, Div, ScrollStrategy, SharedString, UniformListScrollHandle, div,
    prelude::*, px,
};
use skwir_core::app::ClashQuestion;
use skwir_core::op::clash::{Act, Answer, Choice, Clash, FileChoice, FolderChoice, Policy, Side};
use skwir_core::words;
use skwir_core::words::{Name, Size, Words};
use skwir_core::{App, CancelToken};

use crate::controls::{self, ButtonStyle};
use crate::icons::Pictured;
use crate::keymap;
use crate::modal::{self, ListMove, Rows};
use crate::review::{Review, ReviewFocus};
use crate::shell::Shell;
use crate::theme::Theme;

/// The clashes' list: every one, two lines a row (its name, then both
/// sides), only those in sight built.
const ROWS: Rows = Rows {
    height: 48.,
    shown: 7,
};

/// A file's choices for all, and for one (where "if newer" is seen).
const FILES: [Choice; 4] = [
    Choice::KeepBoth,
    Choice::Replace,
    Choice::ReplaceIfNewer,
    Choice::Skip,
];
const FILE: [Choice; 3] = [Choice::KeepBoth, Choice::Replace, Choice::Skip];
const FOLDERS: [Choice; 3] = [Choice::Merge, Choice::KeepBoth, Choice::Skip];

/// A row's picture, in pixels: a list's smallest icons.
const PICTURE: u16 = 16;

/// How often the worker reading the rows' pictures hands over what it
/// read: a question of thousands shows its first rows' at once.
const HAND_OVER: Duration = Duration::from_millis(50);

/// The dialog's state: the question, the choices for all, those made one
/// by one, and the row the keyboard is on.
pub struct Clashes {
    pub question: ClashQuestion,
    files: FileChoice,
    folders: FolderChoice,
    /// Choices made for one clash, by its index.
    each: HashMap<usize, Choice>,
    /// The files' choice for all, the folders' (where one is a folder
    /// onto a folder), then the clashes shown.
    cursor: usize,
    scroll: UniformListScrollHandle,
    /// What each clash brings, pictured as the views picture it, by its
    /// index: none until read (`read_pictures`), or where nothing is.
    pictures: Vec<Option<Pictured>>,
    /// Stops that reading once the question is answered, or handed on
    /// to another window (which reads it again).
    reading: Stops,
    /// The rows above the list ([`Clashes::head_rows`]), said of the whole
    /// question once: every row drawn and every key asks.
    heads: Vec<Row>,
}

/// Cancels its token when it goes.
struct Stops(CancelToken);

impl Drop for Stops {
    fn drop(&mut self) {
        self.0.cancel();
    }
}

fn label(choice: Choice) -> Words {
    match choice {
        Choice::KeepBoth => words!(
            clashes_keep_both = "Keep both",
            note = "A choice for a name already taken where something goes: keep what is there and what comes, the one coming under a name of its own."
        ),
        Choice::Replace => words!(
            clashes_replace = "Replace",
            note = "A choice for a name already taken where something goes: what comes replaces what is there."
        ),
        Choice::ReplaceIfNewer => words!(
            clashes_replace_if_newer = "Replace if newer",
            note = "A choice for a name already taken where something goes: what comes replaces what is there only where it changed later."
        ),
        Choice::Skip => words!(
            clashes_skip = "Skip",
            note = "A choice for a name already taken where something goes: what comes is left where it is."
        ),
        Choice::Merge => words!(
            clashes_merge = "Merge",
            note = "A choice for a folder coming where a folder of its name is: what it holds goes into the one there."
        ),
    }
}

fn file_choice(choice: Choice) -> Option<FileChoice> {
    match choice {
        Choice::KeepBoth => Some(FileChoice::KeepBoth),
        Choice::Replace => Some(FileChoice::Replace),
        Choice::ReplaceIfNewer => Some(FileChoice::ReplaceIfNewer),
        Choice::Skip => Some(FileChoice::Skip),
        Choice::Merge => None,
    }
}

fn folder_choice(choice: Choice) -> Option<FolderChoice> {
    match choice {
        Choice::Merge => Some(FolderChoice::Merge),
        Choice::KeepBoth => Some(FolderChoice::KeepBoth),
        Choice::Skip => Some(FolderChoice::Skip),
        Choice::Replace | Choice::ReplaceIfNewer => None,
    }
}

/// Whether what comes changed later than what is there.
fn newer(clash: &Clash) -> bool {
    matches!((clash.source.modified, clash.there.modified),
        (Some(source), Some(there)) if source > there)
}

impl Clashes {
    pub fn new(question: ClashQuestion) -> Self {
        let clashes = &question.clashes;
        let mut heads = Vec::with_capacity(2);
        if clashes.iter().any(|clash| !clash.folders()) {
            heads.push(Row::Files);
        }
        if clashes.iter().any(Clash::folders) {
            heads.push(Row::Folders);
        }
        Clashes {
            heads,
            pictures: vec![None; question.clashes.len()],
            question,
            files: FileChoice::default(),
            folders: FolderChoice::default(),
            each: HashMap::new(),
            cursor: 0,
            scroll: UniformListScrollHandle::new(),
            reading: Stops(CancelToken::new()),
        }
    }

    /// The rows above the list, the choices for all, of each kind the
    /// question lists: a file's for every clash but a folder's onto a
    /// folder, a folder's for those. A kind none of its clashes is of has
    /// no row, which would choose nothing (a folder merged, all in it
    /// there already or missing). Said of the whole question, never of
    /// the rows shown: a row the user just answered stays where it is.
    fn head_rows(&self) -> &[Row] {
        &self.heads
    }

    fn heads(&self) -> usize {
        self.heads.len()
    }

    /// What the choices for all are called. The first is every clash's
    /// but a folder's onto a folder, named by what comes: a folder onto a
    /// file, or one a step makes that cannot say what it holds (never
    /// merged), takes a file's choices and is a folder all the same. The second, a folder's onto a
    /// folder, says so where the first names folders too.
    fn head_names(&self) -> (Words, Words) {
        let (mut files, mut folders) = (false, false);
        for clash in self.question.clashes.iter().filter(|c| !c.folders()) {
            if clash.source.folder {
                folders = true;
            } else {
                files = true;
            }
        }
        let folders_for_all = || {
            words!(
                clashes_folders_for_all = "Folders, for all",
                note = "The row choosing what becomes of every folder whose name is taken, beside the rows of each."
            )
        };
        let first = match (files, folders) {
            (true, true) => words!(
                clashes_files_and_folders_for_all = "Files and folders, for all",
                note = "The row choosing what becomes of every file and folder whose name is taken (but folders onto folders), beside the rows of each."
            ),
            (false, true) => folders_for_all(),
            _ => words!(
                clashes_files_for_all = "Files, for all",
                note = "The row choosing what becomes of every file whose name is taken, beside the rows of each."
            ),
        };
        let second = if folders {
            words!(
                clashes_folders_onto_folders_for_all = "Folders onto folders, for all",
                note = "The row choosing what becomes of every folder coming where a folder of its name is, beside the rows of each."
            )
        } else {
            folders_for_all()
        };
        (first, second)
    }

    /// What becomes of clash `i`: as chosen for it, else as for all of its
    /// kind.
    fn choice(&self, i: usize) -> Choice {
        let clash = &self.question.clashes[i];
        self.each.get(&i).copied().unwrap_or_else(|| {
            if clash.folders() {
                self.folders.into()
            } else {
                self.files.into()
            }
        })
    }

    /// What clash `i` will do: "replace if newer" said as one or the other.
    fn outcome(&self, i: usize) -> Choice {
        match self.choice(i) {
            Choice::ReplaceIfNewer if newer(&self.question.clashes[i]) => Choice::Replace,
            Choice::ReplaceIfNewer => Choice::Skip,
            choice => choice,
        }
    }

    /// The clashes listed: one in a folder merged, under it.
    fn shown(&self) -> Vec<usize> {
        let mut merged = vec![false; self.question.clashes.len()];
        let mut shown = Vec::new();
        for (i, clash) in self.question.clashes.iter().enumerate() {
            if clash.inside.is_none_or(|parent| merged[parent]) {
                merged[i] = clash.folders() && self.outcome(i) == Choice::Merge;
                shown.push(i);
            }
        }
        shown
    }

    fn rows(&self) -> usize {
        self.heads() + self.shown().len()
    }

    fn move_cursor(&mut self, mv: ListMove) {
        self.cursor = mv.from(self.cursor.min(self.rows() - 1), self.rows());
        if let Some(row) = self.cursor.checked_sub(self.heads()) {
            self.scroll.scroll_to_item(row, ScrollStrategy::Nearest);
        }
    }

    /// The choice of the row the keyboard is on, `delta` along its own.
    fn step_choice(&mut self, delta: isize) {
        let along = |options: &[Choice], now: Choice| {
            let at = options.iter().position(|&c| c == now).unwrap_or(0);
            options[(at.cast_signed() + delta)
                .rem_euclid(options.len().cast_signed())
                .cast_unsigned()]
        };
        let heads = self.heads();
        match self.head_rows().get(self.cursor).copied() {
            Some(Row::Files) => {
                let next = along(&FILES, self.files.into());
                self.files = file_choice(next).unwrap_or_default();
            }
            Some(Row::Folders) => {
                let next = along(&FOLDERS, self.folders.into());
                self.folders = folder_choice(next).unwrap_or_default();
            }
            _ => {
                let shown = self.cursor.checked_sub(heads);
                let Some(i) = shown.and_then(|row| self.shown().get(row).copied()) else {
                    return;
                };
                let options: &[Choice] = if self.question.clashes[i].folders() {
                    &FOLDERS
                } else {
                    &FILE
                };
                let next = along(options, self.outcome(i));
                self.each.insert(i, next);
            }
        }
        self.cursor = self.cursor.min(self.rows() - 1);
    }

    /// The user's answer: the choices for all, and those made one by one.
    pub fn answer(&self) -> Answer {
        Answer {
            policy: Policy {
                files: self.files,
                folders: self.folders,
            },
            each: self
                .each
                .iter()
                .map(|(&i, &choice)| (self.question.clashes[i].key(), choice))
                .collect(),
        }
    }

    /// What the plan does, as its button says it, and that nothing is
    /// done until it is answered: ("Copy", "Nothing is copied until you
    /// choose."), or what a step makes says ("Extract", "Nothing is
    /// extracted…").
    pub fn words(&self) -> (Words, Words) {
        match self.question.clashes.first().map(|c| &c.act) {
            Some(Act::Move) => (
                words!(
                    clashes_move = "Move",
                    note = "The button moving what was asked, once the names taken are answered."
                ),
                words!(
                    clashes_nothing_moved = "Nothing is moved until you choose.",
                    note = "Under the title of the dialog asking what becomes of the names taken where something is moved."
                ),
            ),
            Some(Act::Link) => (
                words!(
                    clashes_link = "Link",
                    note = "The button making the links asked, once the names taken are answered."
                ),
                words!(
                    clashes_nothing_linked = "Nothing is linked until you choose.",
                    note = "Under the title of the dialog asking what becomes of the names taken where links are made."
                ),
            ),
            // What a step makes says it in words of its own.
            Some(Act::Make { words, .. }) => (words.verb.clone(), words.waiting.clone()),
            Some(Act::Copy) | None => (
                words!(
                    clashes_copy = "Copy",
                    note = "The button copying what was asked, once the names taken are answered."
                ),
                words!(
                    clashes_nothing_copied = "Nothing is copied until you choose.",
                    note = "Under the title of the dialog asking what becomes of the names taken where something is copied."
                ),
            ),
        }
    }
}

impl Shell {
    fn clashes_mut(&mut self) -> Option<&mut Clashes> {
        match self.reviews.front_mut() {
            Some(Review::Clashes(clashes)) if self.review_focus.is_some() => Some(clashes),
            _ => None,
        }
    }

    /// The arrows in the dialog: whether it is shown to take them.
    pub fn clashes_move(&mut self, mv: ListMove, cx: &mut Context<Self>) -> bool {
        let Some(clashes) = self.clashes_mut() else {
            return false;
        };
        clashes.move_cursor(mv);
        cx.notify();
        true
    }

    pub fn clashes_choose(&mut self, delta: isize, cx: &mut Context<Self>) {
        if let Some(clashes) = self.clashes_mut() {
            clashes.step_choice(delta);
            cx.notify();
        }
    }

    /// A click on a choice: for one clash, or for all of a kind.
    fn clashes_set(&mut self, row: Row, choice: Choice, cx: &mut Context<Self>) {
        let Some(clashes) = self.clashes_mut() else {
            return;
        };
        match row {
            Row::Files => clashes.files = file_choice(choice).unwrap_or_default(),
            Row::Folders => clashes.folders = folder_choice(choice).unwrap_or_default(),
            Row::Clash(i) => {
                clashes.each.insert(i, choice);
            }
        }
        clashes.cursor = clashes.cursor.min(clashes.rows() - 1);
        cx.notify();
    }
}

/// What a clash's picture is read from: what comes.
enum Wanted {
    /// A file there to copy or move, or to make a link to (`linked`).
    At { path: PathBuf, linked: bool },
    /// What a step makes, not there yet: known by its name.
    Made { name: OsString, folder: bool },
}

impl Wanted {
    fn of(clash: &Clash) -> Self {
        match &clash.act {
            Act::Make { .. } => Wanted::Made {
                name: clash
                    .there
                    .path
                    .file_name()
                    .unwrap_or_default()
                    .to_os_string(),
                folder: clash.source.folder,
            },
            act => Wanted::At {
                path: clash.source.path.clone(),
                linked: *act == Act::Link,
            },
        }
    }

    /// Its picture, read with `statter`: none where nothing is there.
    fn picture(&self, statter: &mut skwir_core::fs::Statter) -> Option<Pictured> {
        match self {
            Wanted::At { path, linked } => {
                let picture = Pictured::of(&statter.stat(path)?, path);
                Some(if *linked { picture.linked() } else { picture })
            }
            Wanted::Made { name, folder } => Some(Pictured::named(name, *folder)),
        }
    }
}

/// Read each of `wanted`'s pictures, handing over what was read every
/// `HAND_OVER`, until `cancel` stops it.
fn picture_each(
    wanted: &[Wanted],
    cancel: &CancelToken,
    mut send: impl FnMut(Vec<(usize, Pictured)>),
) {
    let mut statter = skwir_core::fs::Statter::default();
    let mut read = Vec::new();
    let mut last = Instant::now();
    for (at, wanted) in wanted.iter().enumerate() {
        if cancel.is_cancelled() {
            return;
        }
        read.extend(wanted.picture(&mut statter).map(|picture| (at, picture)));
        if last.elapsed() >= HAND_OVER {
            send(std::mem::take(&mut read));
            last = Instant::now();
        }
    }
    send(read);
}

/// Read on a worker what each of `clashes` brings, pictured as the views
/// picture it, in the lane of what it reads (ADR-077): the rows take
/// their pictures as they come, until the question is answered.
pub(crate) fn read_pictures(app: &Arc<App>, clashes: &mut Clashes, cx: &mut Context<Shell>) {
    let wanted: Vec<Wanted> = clashes.question.clashes.iter().map(Wanted::of).collect();
    if wanted.is_empty() {
        return;
    }
    let cancel = app.lifetime().child();
    clashes.reading = Stops(cancel.clone());
    let id = clashes.question.id;
    let lane = crate::shell::lane_of(wanted.iter().filter_map(|wanted| match wanted {
        Wanted::At { path, .. } => Some(path),
        Wanted::Made { .. } => None,
    }));
    let (tx, rx) = flume::unbounded();
    let token = cancel.clone();
    app.scheduler.spawn_in(
        lane,
        skwir_core::scheduler::Priority::Visible,
        "clash pictures",
        cancel,
        move || {
            picture_each(&wanted, &token, |read| {
                let _ = tx.send(read);
            });
        },
    );
    cx.spawn(async move |this, cx| {
        while let Ok(read) = rx.recv_async().await {
            let asking = this.update(cx, |shell, cx| {
                let Some(clashes) = shell.reviews.iter_mut().find_map(|review| match review {
                    Review::Clashes(clashes) if clashes.question.id == id => Some(clashes),
                    _ => None,
                }) else {
                    return false;
                };
                for (at, picture) in read {
                    if let Some(slot) = clashes.pictures.get_mut(at) {
                        *slot = Some(picture);
                    }
                }
                cx.notify();
                true
            });
            if !matches!(asking, Ok(true)) {
                break;
            }
        }
    })
    .detach();
}

#[derive(Clone, Copy)]
enum Row {
    Files,
    Folders,
    Clash(usize),
}

/// A side of a clash in words: "12 KB, Today 14:02", "folder, 1 Oct".
fn said(side: &Side, now: &jiff::Zoned) -> Words {
    // Transitional: a moment said as text until moments are arguments of
    // their own (ADR-139).
    let time = side.modified.map(|t| crate::format::friendly_time(t, now));
    match (side.folder, side.size, time) {
        (true, _, None) => words!(
            clashes_side_folder = "folder",
            note =
                "One side of a name taken, a folder, under its name: what comes, or what is there."
        ),
        (true, _, Some(time)) => words!(
            clashes_side_folder_at = "folder, { $time }",
            time = time,
            note = "One side of a name taken, a folder, under its name, and when it changed (Today 14:02)."
        ),
        (false, Some(bytes), None) => words!(
            clashes_side_size = "{ $size }",
            size = Size(bytes),
            note = "One side of a name taken, a file, under its name: its size (12 KB)."
        ),
        (false, Some(bytes), Some(time)) => words!(
            clashes_side_size_at = "{ $size }, { $time }",
            size = Size(bytes),
            time = time,
            note = "One side of a name taken, a file, under its name: its size (12 KB) and when it changed (Today 14:02)."
        ),
        (false, None, None) => words!(
            clashes_side_file = "file",
            note = "One side of a name taken, a file whose size is unknown, under its name."
        ),
        (false, None, Some(time)) => words!(
            clashes_side_file_at = "file, { $time }",
            time = time,
            note = "One side of a name taken, a file whose size is unknown, under its name, and when it changed (Today 14:02)."
        ),
    }
}

/// The choices of a row, `selected` lit, each a click away.
fn choices(
    id: SharedString,
    row: Row,
    options: &[Choice],
    selected: Choice,
    theme: &Theme,
    cx: &mut Context<Shell>,
) -> Div {
    options.iter().enumerate().fold(
        div()
            .flex_none()
            .flex()
            .items_center()
            .gap(sized(2.))
            .p(sized(2.))
            .h(sized(controls::HEIGHT))
            .rounded_md()
            .bg(theme.bg)
            .border_1()
            .border_color(theme.border),
        |frame, (n, &choice)| {
            frame.child(
                controls::segment(
                    SharedString::from(format!("{id}-{n}")),
                    label(choice).say(),
                    choice == selected,
                    theme,
                )
                .on_click(cx.listener(move |this, _, _, cx| this.clashes_set(row, choice, cx))),
            )
        },
    )
}

/// The clashes' rows in `range`, the only ones drawn: each its picture
/// and name (under the folder merged it is in), both sides, and its
/// choices; the pictures' icons asked for once the rows are built.
fn clash_rows(shell: &mut Shell, range: Range<usize>, cx: &mut Context<Shell>) -> Vec<AnyElement> {
    let Some(Review::Clashes(clashes)) = shell.reviews.front() else {
        return Vec::new();
    };
    let icons = &mut shell.icons;
    let theme = cx.global::<Theme>().clone();
    let now = jiff::Zoned::now();
    let shown = clashes.shown();
    let mut rows = Vec::with_capacity(range.len());
    for n in range {
        let Some(&i) = shown.get(n) else {
            break;
        };
        let clash = &clashes.question.clashes[i];
        // Where it is, from the folder the plan goes to.
        let mut depth = 0;
        let mut root = clash;
        while let Some(parent) = root.inside {
            depth += 1;
            root = &clashes.question.clashes[parent];
        }
        let base = root.there.path.parent().unwrap_or(&root.there.path);
        let name = clash
            .there
            .path
            .strip_prefix(base)
            .unwrap_or(&clash.there.path)
            .to_string_lossy()
            .into_owned();
        let (yours, theirs) = (said(&clash.source, &now), said(&clash.there, &now));
        let later = newer(clash);
        let earlier = matches!((clash.source.modified, clash.there.modified),
            (Some(coming), Some(staying)) if coming < staying);
        let detail = if later {
            words!(
                clashes_sides_yours_newer = "Yours: { $yours } · newer — There: { $theirs }",
                yours = yours,
                theirs = theirs,
                note = "Under a name taken: what comes (yours), newer, then what is there; each a size or folder and when it changed."
            )
        } else if earlier {
            words!(
                clashes_sides_theirs_newer = "Yours: { $yours } — There: { $theirs } · newer",
                yours = yours,
                theirs = theirs,
                note = "Under a name taken: what comes (yours), then what is there, newer; each a size or folder and when it changed."
            )
        } else {
            words!(
                clashes_sides = "Yours: { $yours } — There: { $theirs }",
                yours = yours,
                theirs = theirs,
                note = "Under a name taken: what comes (yours), then what is there; each a size or folder and when it changed."
            )
        }
        .say();
        let options: &[Choice] = if clash.folders() { &FOLDERS } else { &FILE };
        rows.push(
            div()
                // Known by its clash, not by where it stands.
                .id(("clash-row", i))
                .w_full()
                .h(sized(ROWS.height))
                .flex()
                .items_center()
                .gap_2()
                .px_2()
                .rounded_md()
                .when(clashes.cursor == clashes.heads() + n, |d| d.bg(theme.hover))
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .flex()
                        .flex_col()
                        .pl(px(16. * depth as f32))
                        .child(
                            div()
                                .flex()
                                .items_center()
                                .gap_1()
                                .min_w_0()
                                .text_body()
                                .line_height(sized(20.))
                                .child(
                                    div().size(px(f32::from(PICTURE))).flex_none().children(
                                        clashes.pictures[i]
                                            .as_ref()
                                            .map(|picture| picture.picture(icons, PICTURE, &theme)),
                                    ),
                                )
                                .child(div().min_w_0().truncate().child(name)),
                        )
                        .child(
                            div()
                                .text_small()
                                .line_height(sized(16.))
                                .text_color(theme.text_muted)
                                .truncate()
                                .child(detail),
                        ),
                )
                .child(choices(
                    format!("clash-{i}").into(),
                    Row::Clash(i),
                    options,
                    clashes.outcome(i),
                    &theme,
                    cx,
                ))
                .into_any_element(),
        );
    }
    let app = Arc::clone(&shell.app);
    shell.icons.request(&app);
    rows
}

pub fn render(
    clashes: &Clashes,
    handles: &ReviewFocus,
    pending: usize,
    cx: &mut Context<Shell>,
) -> AnyElement {
    let theme = cx.global::<Theme>().clone();
    let (verb, nothing_done) = clashes.words();
    let first = clashes.question.clashes.first();
    let dest = first
        .and_then(|c| c.there.path.parent())
        .and_then(|p| p.file_name())
        .map_or_else(String::new, |n| n.to_string_lossy().into_owned());
    let top = clashes
        .question
        .clashes
        .iter()
        .filter(|c| c.inside.is_none())
        .count();
    let title = words!(
        clashes_title = "{ $count ->
        [one] { $count } name already in { $folder }
       *[other] { $count } names already in { $folder }
    }",
        count = top,
        folder = Name(dest.clone()),
        note = "The title of the dialog asking what becomes of the names already taken where something goes: count is how many, folder where."
    )
    .say();
    let more = if pending > 0 {
        words!(
            clashes_more_waiting = "{ $count ->
        [one] { $count } more waiting
       *[other] { $count } more waiting
    }",
            count = pending,
            note = "Beside the title of a question of names taken: how many other questions wait after it."
        )
        .say()
    } else {
        String::new()
    };
    let at_cursor = |row: usize| clashes.cursor == row;
    let head = |row: usize,
                name: &Words,
                kind: Row,
                options: &[Choice],
                selected: Choice,
                cx: &mut Context<Shell>| {
        div()
            .flex()
            .items_center()
            .gap_2()
            .px_2()
            .py_1()
            .rounded_md()
            .when(at_cursor(row), |d| d.bg(theme.hover))
            .child(div().flex_1().text_body().child(name.say()))
            .child(choices(
                format!("clash-all-{row}").into(),
                kind,
                options,
                selected,
                &theme,
                cx,
            ))
    };
    let (first, second) = clashes.head_names();
    let mut heads = div().flex().flex_col().gap(sized(2.));
    for (at, &kind) in clashes.head_rows().iter().enumerate() {
        heads = heads.child(match kind {
            Row::Folders => head(at, &second, kind, &FOLDERS, clashes.folders.into(), cx),
            _ => head(at, &first, kind, &FILES, clashes.files.into(), cx),
        });
    }
    let list = ROWS.list(
        "clashes",
        clashes.shown().len(),
        &clashes.scroll,
        &theme,
        |shell, range, _, cx| clash_rows(shell, range, cx),
        cx,
    );
    let panel = modal::panel("clashes", 720., &theme)
        .aria_label(
            words!(
                clashes_named = "Names already taken",
                note = "What the dialog asking what becomes of the names already taken is named to a screen reader."
            )
            .say(),
        )
        .p_3()
        .gap_2()
        .track_focus(&handles.focus)
        .key_context(keymap::REVIEW)
        .child(
            div()
                .flex()
                .items_center()
                .child(div().flex_1().text_title().child(title))
                .child(div().text_small().text_color(theme.text_muted).child(more)),
        )
        .child(
            div()
                .text_body()
                .text_color(theme.text_muted)
                .child(nothing_done.say()),
        )
        .when(
            clashes
                .question
                .clashes
                .iter()
                .any(skwir_core::op::clash::replaces_for_good),
            |d| {
                d.child(
                    div().text_body().text_color(theme.warning).child(
                        words!(
                            clashes_for_good = "{ $folder } is in a network folder, which keeps no trash: what is replaced there is deleted for good.",
                            folder = Name(dest.clone()),
                            note = "In the dialog asking what becomes of the names taken, where what is replaced cannot go to the trash: folder is where it goes."
                        )
                        .say(),
                    ),
                )
            },
        )
        .child(heads)
        .child(div().h(px(1.)).bg(theme.border))
        .child(list)
        .child(modal::footer(
            words!(
                clashes_keys = "↑↓ a row, ←→ its choice · Enter goes on, Escape cancels",
                note = "The keys of the dialog asking what becomes of the names taken, at its foot."
            )
            .say(),
            [
                modal::pressed(
                    controls::button(
                        "review-cancel",
                        words!(
                            clashes_cancel = "Cancel",
                            note = "The button giving up what was asked, rather than answer the names taken."
                        )
                        .say(),
                        None,
                        ButtonStyle::Plain,
                        &handles.cancel,
                        &theme,
                    ),
                    |this, _, cx| this.review_cancel(cx),
                    cx,
                ),
                modal::pressed(
                    controls::button(
                        "review-apply",
                        verb.say(),
                        Some("check"),
                        ButtonStyle::Plain,
                        &handles.apply,
                        &theme,
                    ),
                    |this, _, cx| this.review_apply(cx),
                    cx,
                ),
            ],
            &theme,
        ));
    modal::modal(panel, 80., 6, |_, _, _| {}, &theme, cx)
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;
    use std::time::{Duration, SystemTime};

    use super::*;

    /// What an extraction says of what it makes, as a test's step says it.
    fn extracting() -> std::sync::Arc<skwir_core::op::MakeWords> {
        std::sync::Arc::new(skwir_core::op::MakeWords {
            verb: Words::as_written("Extract"),
            waiting: Words::as_written("Nothing is extracted until you choose."),
            cancelled: Words::as_written("Cancelled: nothing was extracted"),
        })
    }

    fn side(path: &str, folder: bool, secs: u64) -> Side {
        Side {
            path: PathBuf::from(path),
            folder,
            size: (!folder).then_some(10),
            modified: Some(SystemTime::UNIX_EPOCH + Duration::from_secs(secs)),
        }
    }

    fn clash(there: &str, folder: bool, inside: Option<usize>, newer: bool) -> Clash {
        Clash {
            step: 0,
            inside,
            act: Act::Copy,
            source: side("/example/src/x", folder, if newer { 2 } else { 1 }),
            there: side(there, folder, if newer { 1 } else { 2 }),
        }
    }

    fn dialog() -> Clashes {
        Clashes::new(ClashQuestion {
            id: 1,
            title: Words::as_written("copy to /example/dest"),
            clashes: vec![
                clash("/example/dest/a.txt", false, None, true),
                clash("/example/dest/d", true, None, false),
                clash("/example/dest/d/b.txt", false, Some(1), false),
            ],
        })
    }

    /// Each row is pictured by what comes, as the views picture it: a link
    /// as what it leads to, marked; a link made to a file, so too; what an
    /// extraction makes by its name; what is gone, not at all.
    #[cfg(unix)]
    #[test]
    fn each_row_is_pictured_by_what_comes() {
        let tmp = tempfile::tempdir().unwrap();
        let photo = tmp.path().join("it's a photo.jpg");
        std::fs::write(&photo, b"p").unwrap();
        let link = tmp.path().join("to the photo");
        std::os::unix::fs::symlink(&photo, &link).unwrap();
        let archive = tmp.path().join("photos  2026.zip");
        let gone = tmp.path().join("gone.txt");
        let make = Act::Make {
            words: extracting(),
            merges: true,
        };
        let comes = |act: Act, source: &std::path::Path, name: &str, folder: bool| Clash {
            step: 0,
            inside: None,
            act,
            source: Side {
                path: source.to_path_buf(),
                folder,
                size: None,
                modified: None,
            },
            there: side(&format!("/example/dest/{name}"), folder, 1),
        };
        let clashes = [
            comes(Act::Copy, &photo, "it's a photo.jpg", false),
            comes(Act::Move, &link, "to the photo", false),
            comes(Act::Link, &photo, "it's a photo.jpg", false),
            comes(make.clone(), &archive, "day one.txt", false),
            comes(make, &archive, "album", true),
            comes(Act::Copy, &gone, "gone.txt", false),
        ];
        let wanted: Vec<Wanted> = clashes.iter().map(Wanted::of).collect();
        let mut read = Vec::new();
        picture_each(&wanted, &CancelToken::new(), |some| read.extend(some));
        let file = Pictured::of(&skwir_core::fs::stat_path(&photo).unwrap(), &photo);
        assert_eq!(
            read,
            [
                (0, file.clone()),
                (1, file.clone().linked()),
                (2, file.linked()),
                (
                    3,
                    Pictured::named(std::ffi::OsStr::new("day one.txt"), false)
                ),
                (4, Pictured::named(std::ffi::OsStr::new("album"), true)),
            ]
        );
        let cancelled = CancelToken::new();
        cancelled.cancel();
        let mut none = Vec::new();
        picture_each(&wanted, &cancelled, |some| none.extend(some));
        assert!(none.is_empty());
    }

    #[test]
    fn a_folder_merged_lists_its_own_clashes_under_it() {
        let mut d = dialog();
        assert_eq!(d.shown(), vec![0, 1, 2]);
        assert_eq!(d.rows(), 2 + 3, "files and folders for all, then three");
        // The folder skipped, what is in it is never reached.
        d.cursor = 3;
        d.step_choice(1);
        assert_eq!(d.choice(1), Choice::KeepBoth);
        assert_eq!(d.shown(), vec![0, 1]);
        d.step_choice(1);
        assert_eq!(d.choice(1), Choice::Skip);
        assert_eq!(d.rows(), 4);
    }

    #[test]
    fn replace_if_newer_shows_what_it_will_do_and_answers_for_all() {
        let mut d = dialog();
        d.cursor = 0;
        d.step_choice(2);
        assert_eq!(d.files, FileChoice::ReplaceIfNewer);
        assert_eq!(d.outcome(0), Choice::Replace, "newer");
        assert_eq!(d.outcome(2), Choice::Skip, "older");
        // Changed for one, from what it shows.
        d.cursor = 2;
        d.step_choice(1);
        assert_eq!(d.choice(0), Choice::Skip);
        let answer = d.answer();
        assert_eq!(answer.policy.files, FileChoice::ReplaceIfNewer);
        assert_eq!(answer.policy.folders, FolderChoice::Merge);
        assert_eq!(
            answer.each,
            HashMap::from([((0, PathBuf::from("/example/dest/a.txt")), Choice::Skip)])
        );
    }

    /// The names of the choices for all, in English.
    fn english((first, second): (Words, Words)) -> [String; 2] {
        [first.english(), second.english()]
    }

    #[test]
    fn the_choices_for_all_are_named_by_what_comes() {
        assert_eq!(
            english(dialog().head_names()),
            ["Files, for all", "Folders, for all"]
        );
        // A folder an extraction makes that cannot say what it holds takes
        // a file's choices.
        let made = |there: &str, folder: bool| Clash {
            act: Act::Make {
                words: extracting(),
                merges: false,
            },
            ..clash(there, folder, None, true)
        };
        let question = |clashes| {
            Clashes::new(ClashQuestion {
                id: 1,
                title: Words::as_written("extract"),
                clashes,
            })
        };
        let folder = question(vec![made("/example/dest/notes", true)]);
        assert_eq!(folder.heads(), 1);
        assert_eq!(folder.head_names().0.english(), "Folders, for all");
        let both = question(vec![
            made("/example/dest/notes", true),
            made("/example/dest/report.pdf", false),
        ]);
        assert_eq!(both.head_names().0.english(), "Files and folders, for all");
        // Beside folders merged, each says which folders it is for.
        let beside = question(vec![
            made("/example/dest/notes", true),
            clash("/example/dest/d", true, None, false),
        ]);
        assert_eq!(beside.heads(), 2);
        assert_eq!(
            english(beside.head_names()),
            ["Folders, for all", "Folders onto folders, for all"]
        );
        // One that says what it holds is merged, as a copy's folder is:
        // its own clashes under it.
        let merged = |there: &str, folder: bool, inside| Clash {
            act: Act::Make {
                words: extracting(),
                merges: true,
            },
            ..clash(there, folder, inside, true)
        };
        let mut into = question(vec![
            merged("/example/dest/photos", true, None),
            merged("/example/dest/photos/it's a.jpg", false, Some(0)),
        ]);
        assert_eq!(into.heads(), 2);
        let (verb, nothing_done) = into.words();
        assert_eq!(verb.english(), "Extract");
        assert_eq!(
            nothing_done.english(),
            "Nothing is extracted until you choose."
        );
        assert_eq!(into.outcome(0), Choice::Merge);
        assert_eq!(into.shown(), vec![0, 1]);
        into.each.insert(0, Choice::KeepBoth);
        assert_eq!(into.shown(), vec![0], "kept beside, nothing in it asked");
        // The row for all of a kind stays, its clash listed but hidden.
        assert_eq!(into.heads(), 2);
    }

    /// A folder merged, nothing in it asked about (all there already, or
    /// missing): no row of files' choices, which would choose nothing.
    #[test]
    fn a_kind_no_clash_is_of_has_no_row_for_all() {
        let mut d = Clashes::new(ClashQuestion {
            id: 1,
            title: Words::as_written("copy to /example/dest"),
            clashes: vec![clash("/example/dest/photos", true, None, false)],
        });
        assert_eq!(d.heads(), 1);
        assert_eq!(d.head_names().1.english(), "Folders, for all");
        assert_eq!(d.rows(), 2);
        // The first row is the folders' choice for all.
        d.step_choice(1);
        assert_eq!(d.answer().policy.folders, FolderChoice::KeepBoth);
        assert_eq!(d.answer().policy.files, FileChoice::KeepBoth);
        d.move_cursor(ListMove::Last);
        d.step_choice(1);
        assert_eq!(d.outcome(0), Choice::Skip);
    }

    /// Every name taken is a row the keys reach, however many: the list
    /// scrolls, never cut at a count.
    #[test]
    fn every_name_taken_is_listed() {
        let clashes = (0..1_000)
            .map(|n| clash(&format!("/example/dest/it's {n}.txt"), false, None, true))
            .collect();
        let mut d = Clashes::new(ClashQuestion {
            id: 1,
            title: Words::as_written("copy to /example/dest"),
            clashes,
        });
        assert_eq!(d.shown().len(), 1_000);
        assert_eq!(d.rows(), 1 + 1_000);
        d.move_cursor(ListMove::Last);
        assert_eq!(d.cursor, 1_000);
        d.step_choice(1);
        assert_eq!(
            d.choice(999),
            Choice::Replace,
            "the last one chosen for itself"
        );
    }

    #[test]
    fn the_arrows_go_round_the_rows() {
        let mut d = dialog();
        d.move_cursor(ListMove::By(-1));
        assert_eq!(d.cursor, 4);
        d.move_cursor(ListMove::By(1));
        assert_eq!(d.cursor, 0);
    }
}
