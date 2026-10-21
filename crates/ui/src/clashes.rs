//! What becomes of the names a plan found taken where it copies, moves,
//! links or makes something (ADR-054, ADR-059): one dialog lists them all,
//! each choice made for one
//! item or for all of a kind, answered before anything is done. It waits
//! in the review queue (`review.rs`) and is answered as a review is: Enter
//! goes on, Escape cancels; the arrows walk its rows and change their
//! choice.

use std::collections::HashMap;

use gpui::{AnyElement, Context, Div, ScrollHandle, SharedString, div, prelude::*, px};
use skwir_core::app::ClashQuestion;
use skwir_core::op::clash::{Act, Answer, Choice, Clash, FileChoice, FolderChoice, Policy, Side};

use crate::controls::{self, ButtonStyle};
use crate::keymap;
use crate::modal::{self, ListMove};
use crate::review::{Review, ReviewFocus};
use crate::shell::Shell;
use crate::theme::Theme;

/// The clashes listed at most: the others follow the choice for all.
const SHOWN: usize = 200;

/// A file's choices for all, and for one (where "if newer" is seen).
const FILES: [Choice; 4] = [
    Choice::KeepBoth,
    Choice::Replace,
    Choice::ReplaceIfNewer,
    Choice::Skip,
];
const FILE: [Choice; 3] = [Choice::KeepBoth, Choice::Replace, Choice::Skip];
const FOLDERS: [Choice; 3] = [Choice::Merge, Choice::KeepBoth, Choice::Skip];

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
    scroll: ScrollHandle,
}

fn label(choice: Choice) -> &'static str {
    match choice {
        Choice::KeepBoth => "Keep both",
        Choice::Replace => "Replace",
        Choice::ReplaceIfNewer => "Replace if newer",
        Choice::Skip => "Skip",
        Choice::Merge => "Merge",
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
        Clashes {
            question,
            files: FileChoice::default(),
            folders: FolderChoice::default(),
            each: HashMap::new(),
            cursor: 0,
            scroll: ScrollHandle::new(),
        }
    }

    fn any_folders(&self) -> bool {
        self.question.clashes.iter().any(Clash::folders)
    }

    /// The rows above the list: the choices for all.
    fn heads(&self) -> usize {
        1 + usize::from(self.any_folders())
    }

    /// What the choices for all are called. The first is every clash's
    /// but a folder's onto a folder, named by what comes: a folder onto a
    /// file, or one a step makes (never merged), takes a file's choices
    /// and is a folder all the same. The second, a folder's onto a
    /// folder, says so where the first names folders too.
    fn head_names(&self) -> (&'static str, &'static str) {
        let (mut files, mut folders) = (false, false);
        for clash in self.question.clashes.iter().filter(|c| !c.folders()) {
            if clash.source.folder {
                folders = true;
            } else {
                files = true;
            }
        }
        let first = match (files, folders) {
            (true, true) => "Files and folders, for all",
            (false, true) => "Folders, for all",
            _ => "Files, for all",
        };
        let second = if folders {
            "Folders onto folders, for all"
        } else {
            "Folders, for all"
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
        self.heads() + self.shown().len().min(SHOWN)
    }

    fn move_cursor(&mut self, mv: ListMove) {
        self.cursor = mv.from(self.cursor.min(self.rows() - 1), self.rows());
        if let Some(row) = self.cursor.checked_sub(self.heads()) {
            self.scroll.scroll_to_item(row);
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
        match self.cursor {
            0 => {
                let next = along(&FILES, self.files.into());
                self.files = file_choice(next).unwrap_or_default();
            }
            1 if heads == 2 => {
                let next = along(&FOLDERS, self.folders.into());
                self.folders = folder_choice(next).unwrap_or_default();
            }
            row => {
                let Some(&i) = self.shown().get(row - heads) else {
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

    /// What the plan does, as its button says it and as what is not done
    /// until it is answered: ("Copy", "copied"), ("Move", "moved"),
    /// ("Link", "linked"), or what a step makes says ("Extract",
    /// "extracted").
    pub fn words(&self) -> (&'static str, &'static str) {
        match self.question.clashes.first().map(|c| c.act) {
            Some(Act::Move) => ("Move", "moved"),
            Some(Act::Link) => ("Link", "linked"),
            Some(Act::Make { verb, done }) => (verb, done),
            Some(Act::Copy) | None => ("Copy", "copied"),
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

#[derive(Clone, Copy)]
enum Row {
    Files,
    Folders,
    Clash(usize),
}

/// A side of a clash in words: "12 KB, Today 14:02", "folder, 1 Oct".
fn said(side: &Side, now: &jiff::Zoned) -> String {
    let what = match side.size {
        _ if side.folder => "folder".to_owned(),
        Some(bytes) => crate::format::size(bytes),
        None => "file".to_owned(),
    };
    match side.modified {
        Some(t) => format!("{what}, {}", crate::format::friendly_time(t, now)),
        None => what,
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
            .gap(px(2.))
            .p(px(2.))
            .h(px(controls::HEIGHT))
            .rounded_md()
            .bg(theme.bg)
            .border_1()
            .border_color(theme.border),
        |frame, (n, &choice)| {
            frame.child(
                controls::segment(
                    SharedString::from(format!("{id}-{n}")),
                    label(choice),
                    choice == selected,
                    theme,
                )
                .on_click(cx.listener(move |this, _, _, cx| this.clashes_set(row, choice, cx))),
            )
        },
    )
}

pub fn render(
    clashes: &Clashes,
    handles: &ReviewFocus,
    pending: usize,
    cx: &mut Context<Shell>,
) -> AnyElement {
    let theme = cx.global::<Theme>().clone();
    let now = jiff::Zoned::now();
    let (verb, done) = clashes.words();
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
    let title = format!("{} already in “{dest}”", crate::format::count(top, "name"));
    let more = if pending > 0 {
        format!("{pending} more waiting")
    } else {
        String::new()
    };
    let at_cursor = |row: usize| clashes.cursor == row;
    let head = |row: usize,
                name: &str,
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
            .child(div().flex_1().text_sm().child(name.to_owned()))
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
    let mut heads = div().flex().flex_col().gap(px(2.)).child(head(
        0,
        first,
        Row::Files,
        &FILES,
        clashes.files.into(),
        cx,
    ));
    if clashes.heads() == 2 {
        heads = heads.child(head(
            1,
            second,
            Row::Folders,
            &FOLDERS,
            clashes.folders.into(),
            cx,
        ));
    }
    let shown = clashes.shown();
    let mut list = div()
        .id("clashes")
        .track_scroll(&clashes.scroll)
        .overflow_y_scroll()
        .max_h(px(360.))
        .flex()
        .flex_col()
        .gap(px(2.));
    for (n, &i) in shown.iter().take(SHOWN).enumerate() {
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
        let newer_mark = |is: bool| if is { " · newer" } else { "" };
        let later = newer(clash);
        let earlier = matches!((clash.source.modified, clash.there.modified),
            (Some(coming), Some(staying)) if coming < staying);
        let detail = format!(
            "Yours: {yours}{} — There: {theirs}{}",
            newer_mark(later),
            newer_mark(earlier)
        );
        let options: &[Choice] = if clash.folders() { &FOLDERS } else { &FILE };
        list = list.child(
            div()
                .flex()
                .items_center()
                .gap_2()
                .px_2()
                .py_1()
                .rounded_md()
                .when(at_cursor(clashes.heads() + n), |d| d.bg(theme.hover))
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
                                .text_sm()
                                .when(clash.source.folder, |d| {
                                    d.child(controls::glyph("folder", theme.text_muted, 14.))
                                })
                                .child(div().min_w_0().truncate().child(name)),
                        )
                        .child(
                            div()
                                .text_xs()
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
                )),
        );
    }
    if shown.len() > SHOWN {
        list = list.child(
            div()
                .px_2()
                .text_xs()
                .text_color(theme.text_muted)
                .child(format!(
                    "… and {} more, as chosen for all",
                    crate::format::thousands(shown.len() - SHOWN)
                )),
        );
    }
    let panel = modal::panel("clashes", 720., &theme)
        .aria_label("Names already taken")
        .p_3()
        .gap_2()
        .track_focus(&handles.focus)
        .key_context(keymap::REVIEW)
        .child(
            div()
                .flex()
                .items_center()
                .child(div().flex_1().child(title))
                .child(div().text_xs().text_color(theme.text_muted).child(more)),
        )
        .child(
            div()
                .text_xs()
                .text_color(theme.text_muted)
                .child(format!("Nothing is {done} until you choose.")),
        )
        .when(
            clashes
                .question
                .clashes
                .iter()
                .any(skwir_core::op::clash::replaces_for_good),
            |d| {
                d.child(div().text_xs().text_color(theme.warning).child(format!(
                    "“{dest}” is in a network folder, which keeps no trash: \
                     what is replaced there is deleted for good."
                )))
            },
        )
        .child(heads)
        .child(div().h(px(1.)).bg(theme.border))
        .child(list)
        .child(modal::footer(
            "↑↓ a row, ←→ its choice · Enter goes on, Escape cancels",
            [
                controls::button(
                    "review-cancel",
                    "Cancel",
                    None,
                    ButtonStyle::Plain,
                    &handles.cancel,
                    &theme,
                )
                .on_click(cx.listener(|this, _, _, cx| this.review_cancel(cx))),
                controls::button(
                    "review-apply",
                    verb,
                    Some("check"),
                    ButtonStyle::Plain,
                    &handles.apply,
                    &theme,
                )
                .on_click(cx.listener(|this, _, _, cx| this.review_apply(cx))),
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
            title: "copy to /example/dest".into(),
            clashes: vec![
                clash("/example/dest/a.txt", false, None, true),
                clash("/example/dest/d", true, None, false),
                clash("/example/dest/d/b.txt", false, Some(1), false),
            ],
        })
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

    #[test]
    fn the_choices_for_all_are_named_by_what_comes() {
        assert_eq!(
            dialog().head_names(),
            ("Files, for all", "Folders, for all")
        );
        // A folder an extraction makes takes a file's choices.
        let made = |there: &str, folder: bool| Clash {
            act: Act::Make {
                verb: "Extract",
                done: "extracted",
            },
            ..clash(there, folder, None, true)
        };
        let question = |clashes| {
            Clashes::new(ClashQuestion {
                id: 1,
                title: "extract".into(),
                clashes,
            })
        };
        let folder = question(vec![made("/example/dest/notes", true)]);
        assert_eq!(folder.heads(), 1);
        assert_eq!(folder.head_names().0, "Folders, for all");
        let both = question(vec![
            made("/example/dest/notes", true),
            made("/example/dest/report.pdf", false),
        ]);
        assert_eq!(both.head_names().0, "Files and folders, for all");
        // Beside folders merged, each says which folders it is for.
        let beside = question(vec![
            made("/example/dest/notes", true),
            clash("/example/dest/d", true, None, false),
        ]);
        assert_eq!(beside.heads(), 2);
        assert_eq!(
            beside.head_names(),
            ("Folders, for all", "Folders onto folders, for all")
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
