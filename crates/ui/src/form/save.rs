//! A file to write (`Kind::Save`): one line holding a whole path, written
//! from `~`, folder and name together (Print…'s Save as).
//!
//! - **Opened**, it holds the folder the asker gives and the name its
//!   `names` give for what the form's other fields hold, the name but its
//!   ending selected: typing renames, the ending kept.
//! - **The name follows** the form's fields (a booklet's name once
//!   Booklet is chosen) while it is the one put there; one typed stays.
//! - **Its list is a popover** under it, over the fields below, scrolled
//!   to keep the row highlighted in view, folded once the field loses the
//!   keyboard (but to its own button), and by Escape. Asked for (its
//!   button, Down while it is folded), it offers the asker's folders.
//!   Edited since the window put its text, it completes the folder part a
//!   name at a time,
//!   as the address field completes a path (`complete`, ADR-070): the
//!   folder typed is read once, on a worker in its lane, and each key
//!   matches its folders in memory; nothing completing it, it is folded.
//!   Choosing one (Down then Enter, a
//!   click, Tab) puts it in place of the folder typed and keeps the name:
//!   the one typed, or, where what was typed began that folder's name,
//!   the one put there before; Ctrl+Z takes it back. Enter with nothing
//!   highlighted takes the text. Tab, as in the address field, takes the
//!   row highlighted, else fills in the start the folders completing it
//!   share, else takes the one there is, else highlights the first.
//!   Alt+Up goes to the folder above, the name kept.
//! - **Leaving it** with a folder and no name (`~/Documents/`, or a
//!   folder's own name typed without its separator), the name put there
//!   comes back after it.
//! - **Under it** is said what the folder read says: a folder that is
//!   none, a relative one or a name no file can have, which hold the
//!   form's answer back (`FormState::blocked`); a name it holds already
//!   (saving then asks whether to replace it, `op::clash`); where the PDF
//!   goes when no folder or no name is typed; that a folder alone is
//!   held; else the field's help. A read that did not answer (let go of
//!   for the next, no right to list the folder, a server that failed) says
//!   nothing against the folder and is asked again (`Reads`).
use crate::theme::Chosen;
use std::cell::RefCell;
use std::collections::HashMap;
use std::ops::Range;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::Arc;

use gpui::{AnyElement, Context, SharedString, Window, div, prelude::*};
use skwir_core::App;
use skwir_core::cancel::CancelToken;
use skwir_core::form::path;
use skwir_core::form::{Field, Kind};
use skwir_core::scheduler::Priority;

use super::{Control, FormState};
use crate::complete;
use crate::modal;
use crate::shell::Shell;
use crate::sizes::{TextSizes, sized};
use crate::theme::Theme;

/// A row of the field's list: a folder put in place of the one typed.
pub(super) struct Row {
    pub title: String,
    pub detail: String,
    /// The heading it is listed under ("Chosen before", "Places"), the
    /// asker's `Choice::section`; empty for none.
    pub section: String,
    pub folder: PathBuf,
    /// Completed from what was typed (a folder within the one typed),
    /// not one the asker offers.
    pub completed: bool,
}

/// What a read of a folder typed found (or could not tell).
pub(super) struct Known {
    folder: PathBuf,
    resolved: complete::Resolved,
    /// What the folder itself holds, each name with whether it is a
    /// folder: what is said under the field asks it at each draw, of a
    /// folder of any size. `None` where it is no folder (`resolved` then
    /// holds the folders it may be short for).
    names: Option<HashMap<String, bool>>,
}

impl Known {
    /// Read `folder` for completion. On a worker, in its lane.
    fn read(app: &App, folder: PathBuf, stop: &CancelToken) -> Self {
        let resolved = complete::folders(app, &folder, false, stop);
        Known::of(folder, resolved)
    }

    fn of(folder: PathBuf, resolved: complete::Resolved) -> Self {
        let names = resolved
            .folders
            .first()
            .filter(|holder| !resolved.abbreviated && holder.path == folder)
            .map(|holder| {
                holder
                    .names
                    .iter()
                    .map(|name| (name.text.clone(), name.dir))
                    .collect()
            });
        Known {
            folder,
            resolved,
            names,
        }
    }
}

/// What completes a field's text, kept for what it was worked out from:
/// a draw asks it several times, each key once, and it walks a whole
/// folder.
struct Completed {
    text: String,
    ending: String,
    version: u64,
    rows: Rc<[Row]>,
    shared: Option<String>,
}

/// What a form's Save fields know of the folders they read, and which is
/// being read: one at a time, the read before it let go of. A folder is
/// known once it answered, and only then: nothing else is an answer. A
/// read let go of, or one that could not tell (`Resolved::unread`), leaves
/// nothing behind, so that it says nothing against the folder and the next
/// look reads it again.
#[derive(Default)]
pub(super) struct Reads {
    known: HashMap<PathBuf, Known>,
    /// Moves as `known` changes.
    version: u64,
    /// The folder a read is under way for.
    asked: Option<PathBuf>,
    completed: RefCell<Option<Completed>>,
}

/// The folders kept known for one form, at most.
const KEPT: usize = 16;

/// The folders completing what was typed that the popover lists, at
/// most: it scrolls.
const LISTED: usize = 64;

/// The popover's height at most, at 100 %: about seven rows of two lines.
const POPOVER: f32 = 300.;

/// What is said under the field: whether it warns, and whether it holds
/// the form's answer back (a path no PDF can be saved at).
pub(super) struct Note {
    pub text: String,
    pub warns: bool,
    pub blocks: bool,
}

impl Note {
    fn says(text: impl Into<String>) -> Note {
        Note {
            text: text.into(),
            warns: false,
            blocks: false,
        }
    }

    fn warns(text: impl Into<String>) -> Note {
        Note {
            text: text.into(),
            warns: true,
            blocks: false,
        }
    }

    fn blocks(text: impl Into<String>) -> Note {
        Note {
            text: text.into(),
            warns: true,
            blocks: true,
        }
    }
}

/// `text` split where completion splits it: its folder part, with its
/// last separator, and its name; `~` alone is the home folder.
fn split(text: &str) -> (&str, &str) {
    path::save_split(text)
}

/// The folder `text`'s folder part names, as the asker reads it
/// (`form::path`): `None` without one, or for a relative one.
fn folder_of(text: &str) -> Option<PathBuf> {
    path::save_folder(split(text).0, skwir_core::os_path::home().as_deref())
}

/// `name` without `ending` (case ignored) where it ends so.
fn stem<'a>(name: &'a str, ending: &str) -> &'a str {
    let cut = name.len().saturating_sub(ending.len());
    if !ending.is_empty()
        && name.len() > ending.len()
        && name.is_char_boundary(cut)
        && name[cut..].eq_ignore_ascii_case(ending)
    {
        &name[..cut]
    } else {
        name
    }
}

/// The bytes of `text`'s name but its ending: what is selected once a
/// name is put there, so that typing renames it.
fn stem_range(text: &str, ending: &str) -> Range<usize> {
    let (dir, name) = split(text);
    dir.len()..dir.len() + stem(name, ending).len()
}

/// `name` with `ending` where it has none: what the asker saves it as.
fn ended(name: &str, ending: &str) -> String {
    if ending.is_empty() || stem(name, ending).len() < name.len() {
        name.to_owned()
    } else {
        format!("{name}{ending}")
    }
}

/// `folder` written as the field holds folders: from `~`, ending with
/// its separator.
fn written(folder: &Path) -> String {
    let mut text = skwir_core::os_path::from_home(folder);
    if !text.ends_with(complete::is_separator) {
        text.push(std::path::MAIN_SEPARATOR);
    }
    text
}

/// Whether `text` holds a folder alone: nothing after its last separator,
/// or nothing at all.
fn folder_alone(text: &str) -> bool {
    split(text).1.is_empty()
}

/// What the Save control at `at` holds: its field's ending, its text, the
/// name put there, and whether it holds a folder alone (none of its
/// `names` holding for what the form's fields hold).
struct Held {
    ending: String,
    text: String,
    settled: String,
    alone: bool,
}

impl FormState {
    /// What the Save field `at` answers: its text, the name put there
    /// added when it holds a folder alone.
    pub(super) fn save_value(text: &str, settled: &str) -> String {
        let text = text.trim();
        if folder_alone(text) {
            path::save_join(text, settled)
        } else {
            text.to_owned()
        }
    }

    fn held(&self, at: usize, cx: &gpui::App) -> Option<Held> {
        let (Some(field), Some(Control::Save { input, settled, .. })) =
            (self.form.fields.get(at), self.controls.get(at))
        else {
            return None;
        };
        let Kind::Save { ending, .. } = &field.kind else {
            return None;
        };
        Some(Held {
            ending: ending.clone(),
            text: input.read(cx).text().trim().to_owned(),
            settled: settled.clone(),
            alone: field.save_name(&self.values(cx)).is_empty(),
        })
    }

    /// The rows of the Save field `at`'s list: the asker's folders when
    /// asked for (`offered`), else what completes the folder typed once
    /// its text was edited, else none.
    pub(super) fn save_rows(&self, at: usize, cx: &gpui::App) -> Rc<[Row]> {
        let (Some(field), Some(Control::Save { input, put, .. })) =
            (self.form.fields.get(at), self.controls.get(at))
        else {
            return Rc::from([]);
        };
        let Kind::Save {
            folders, ending, ..
        } = &field.kind
        else {
            return Rc::from([]);
        };
        let text = input.read(cx).text();
        if !self.offered {
            return if text == put.as_str() {
                Rc::from([])
            } else {
                self.read
                    .completions(text, ending)
                    .map_or_else(|| Rc::from([]), |(rows, _)| rows)
            };
        }
        let home = skwir_core::os_path::home();
        folders
            .iter()
            .filter_map(|choice| {
                let folder = complete::resolve(&choice.value, None, home.as_deref())?;
                Some(Row {
                    title: choice.shown().to_owned(),
                    detail: choice.detail.clone(),
                    section: choice.section.clone(),
                    folder,
                    completed: false,
                })
            })
            .collect()
    }

    /// Whether Tab completes in the Save field that has the keyboard: its
    /// list shows folders completing what was typed.
    pub(crate) fn completes(&self, window: &Window, cx: &gpui::App) -> bool {
        self.save_focused(window, cx).is_some_and(|at| {
            self.open == Some(at) && self.save_rows(at, cx).iter().any(|row| row.completed)
        })
    }

    /// The Save field whose text has the keyboard.
    pub(super) fn save_focused(&self, window: &Window, cx: &gpui::App) -> Option<usize> {
        self.controls.iter().position(|control| {
            matches!(control, Control::Save { input, .. } if input.read(cx).is_focused(window))
        })
    }

    /// Why the form's answer is held back: a Save field shown whose path
    /// no file can be saved at (`Note::blocks`).
    pub(super) fn blocked(&self, cx: &gpui::App) -> Option<String> {
        let values = self.values(cx);
        self.form.fields.iter().enumerate().find_map(|(at, field)| {
            if !matches!(field.kind, Kind::Save { .. }) || !self.shows(field, &values) {
                return None;
            }
            self.save_note(at, field, cx)
                .filter(|note| note.blocks)
                .map(|note| note.text)
        })
    }

    /// What is said under the Save field `at` (`Note`), `None` for its
    /// help.
    pub(super) fn save_note(&self, at: usize, field: &Field, cx: &gpui::App) -> Option<Note> {
        let Kind::Save { alone, beside, .. } = &field.kind else {
            return None;
        };
        self.read.note(&self.held(at, cx)?, alone, beside)
    }

    /// Take what the read under way found, if it answered: whether it did.
    pub(super) fn save_took(&mut self) -> bool {
        let Some(known) = self.reading.take() else {
            return false;
        };
        self.read.answered(known);
        true
    }
}

impl Reads {
    /// Whether `folder` is to be read now, noted as being read if so: it
    /// is neither known nor being read.
    fn ask(&mut self, folder: &Path) -> bool {
        if self.known.contains_key(folder) || self.asked.as_deref() == Some(folder) {
            return false;
        }
        if self.known.len() >= KEPT {
            self.known.clear();
            self.version += 1;
        }
        self.asked = Some(folder.to_owned());
        true
    }

    /// The read asked last answered with `known`.
    fn answered(&mut self, known: Known) {
        self.asked = None;
        if !known.resolved.unread {
            self.known.insert(known.folder.clone(), known);
            self.version += 1;
        }
    }

    /// What is known of the folder `text`'s folder part names.
    fn known_of(&self, text: &str) -> Option<&Known> {
        self.known.get(&folder_of(text)?)
    }

    /// Whether `text`'s name, after its folder, is one of that folder's
    /// own folders: `None` while the folder is not known.
    fn names_a_folder(&self, text: &str) -> Option<bool> {
        let (_, name) = split(text);
        if name.is_empty() {
            return Some(false);
        }
        let names = self.known_of(text)?.names.as_ref()?;
        Some(names.get(name) == Some(&true))
    }

    /// Whether the folder `text`'s folder part names is one: `None` while
    /// it is not known.
    fn folder_is(&self, text: &str) -> Option<bool> {
        let known = self.known_of(text)?;
        Some(
            !known.resolved.abbreviated
                && known
                    .resolved
                    .folders
                    .first()
                    .is_some_and(|holder| holder.path == known.folder),
        )
    }

    /// What is said under a Save field holding `held` (`Note`), `None` for
    /// its help; `alone` and `beside` are what its asker says of a folder
    /// alone and of a name with no folder.
    fn note(&self, held: &Held, alone: &str, beside: &str) -> Option<Note> {
        let text = held.text.as_str();
        let (dir, name) = split(text);
        if text.is_empty() || (dir.is_empty() && !held.alone) {
            return (!beside.is_empty()).then(|| Note::says(beside));
        }
        if folder_of(text).is_none() {
            return Some(Note::blocks(
                "Type the folder's whole path, from the top or from ~",
            ));
        }
        let missing = |path: &str| {
            Some(Note::blocks(format!(
                "There is no folder “{}”",
                path.trim_end_matches(complete::is_separator)
            )))
        };
        if self.folder_is(text) == Some(false) {
            return missing(dir);
        }
        if held.alone {
            // The whole text is the folder.
            return match self.names_a_folder(text) {
                // `.` and `..` are folders no listing names.
                Some(false) if !name.is_empty() && !matches!(name, "." | "..") => missing(text),
                _ => (!alone.is_empty()).then(|| Note::says(alone)),
            };
        }
        if name.is_empty() {
            return Some(Note::says(format!("Named “{}”", held.settled)));
        }
        if self.names_a_folder(text) == Some(true) {
            return Some(Note::says(format!(
                "Saved in the folder “{name}”, named “{}”",
                held.settled
            )));
        }
        let full = ended(name, &held.ending);
        if let Err(refused) = skwir_core::op::valid_file_name(std::ffi::OsStr::new(&full)) {
            return Some(Note::blocks(format!("“{full}” cannot be saved: {refused}")));
        }
        if self.known_of(text)?.names.as_ref()?.contains_key(&full) {
            return Some(Note::warns(format!(
                "“{full}” is there already: saving asks whether to replace it"
            )));
        }
        None
    }

    /// The folders within the folder `text` names that its name typed
    /// begins (its ending aside), best first, and the start they share
    /// beyond what was typed; `None` while the folder is not known.
    fn completions(&self, text: &str, ending: &str) -> Option<(Rc<[Row]>, Option<String>)> {
        let resolved = &self.known_of(text)?.resolved;
        let mut completed = self.completed.borrow_mut();
        if let Some(kept) = completed.as_ref().filter(|kept| {
            kept.version == self.version && kept.text == text && kept.ending == ending
        }) {
            return Some((Rc::clone(&kept.rows), kept.shared.clone()));
        }
        let typed = stem(split(text).1, ending);
        let (found, shared) =
            complete::rank_at_most(&resolved.folders, typed, false, LISTED, |name| name.dir);
        // Where each is said only when they come from several folders (a
        // folder part abbreviated): they are all in the one typed else.
        let several = resolved.folders.len() > 1;
        let rows: Rc<[Row]> = found
            .into_iter()
            .map(|m| {
                let holder = &resolved.folders[m.folder];
                let name = &holder.names[m.index];
                Row {
                    title: name.text.clone(),
                    detail: if several {
                        skwir_core::os_path::from_home(&holder.path)
                    } else {
                        String::new()
                    },
                    section: String::new(),
                    folder: holder.path.join(&name.exact),
                    completed: true,
                }
            })
            .collect();
        *completed = Some(Completed {
            text: text.to_owned(),
            ending: ending.to_owned(),
            version: self.version,
            rows: Rc::clone(&rows),
            shared: shared.clone(),
        });
        Some((rows, shared))
    }
}

impl Shell {
    /// The Save fields as a form opens: each holding the name its `names`
    /// give where its value is a folder alone, the name but its ending
    /// selected, its folder read.
    pub(super) fn save_open(&mut self, cx: &mut Context<Self>) {
        let Some(form) = self.form.as_mut() else {
            return;
        };
        let values = form.values(cx);
        let mut opened = Vec::new();
        for (at, (field, control)) in form.form.fields.iter().zip(&mut form.controls).enumerate() {
            let (
                Kind::Save { ending, .. },
                Control::Save {
                    input,
                    settled,
                    put,
                    ..
                },
            ) = (&field.kind, control)
            else {
                continue;
            };
            field.save_name(&values).clone_into(settled);
            let text = FormState::save_value(input.read(cx).text(), settled);
            let range = stem_range(&text, ending);
            text.clone_into(put);
            input.update(cx, |input, cx| {
                input.set_text(text, cx);
                input.select_range(range, cx);
            });
            opened.push(at);
        }
        for at in opened {
            self.save_read(at, cx);
        }
    }

    /// The form's fields changed (a layout chosen): each Save field's name
    /// follows what its `names` give, while it holds the one put there.
    pub(super) fn save_follow(&mut self, cx: &mut Context<Self>) {
        let Some(form) = self.form.as_mut() else {
            return;
        };
        let values = form.values(cx);
        for (field, control) in form.form.fields.iter().zip(&mut form.controls) {
            let (
                Kind::Save { ending, .. },
                Control::Save {
                    input,
                    settled,
                    put,
                    ..
                },
            ) = (&field.kind, control)
            else {
                continue;
            };
            let name = field.save_name(&values);
            if name == settled.as_str() {
                continue;
            }
            let text = input.read(cx).text().to_owned();
            let (dir, typed) = split(&text);
            // One typed stays.
            if typed != settled.as_str() {
                continue;
            }
            let followed = path::save_join(dir, name);
            let range = stem_range(&followed, ending);
            // Put by the window, as the one it follows was: no edit.
            if text == *put {
                followed.clone_into(put);
            }
            input.update(cx, |input, cx| {
                input.set_text(followed, cx);
                input.select_range(range, cx);
            });
            name.clone_into(settled);
        }
    }

    /// Put `text` in the Save field `at` as the window's (no edit of the
    /// user's: its list offers folders again), the name but its ending
    /// selected, as an edit Ctrl+Z takes back; its folder read.
    fn save_put(&mut self, at: usize, text: String, cx: &mut Context<Self>) {
        let Some(form) = self.form.as_mut() else {
            return;
        };
        let (Some(field), Some(Control::Save { input, put, .. })) =
            (form.form.fields.get(at), form.controls.get_mut(at))
        else {
            return;
        };
        let Kind::Save { ending, .. } = &field.kind else {
            return;
        };
        let range = stem_range(&text, ending);
        text.clone_into(put);
        input.update(cx, |input, cx| {
            input.replace_all(&text, cx);
            input.select_range(range, cx);
        });
        self.save_read(at, cx);
    }

    /// Put `row`'s folder in place of the one the Save field `at` holds,
    /// keeping its name (see the module's docs); its list folded.
    pub(super) fn save_choose(&mut self, at: usize, row: &Row, cx: &mut Context<Self>) {
        let Some(form) = self.form.as_mut() else {
            return;
        };
        let Some(held) = form.held(at, cx) else {
            return;
        };
        let (_, name) = split(&held.text);
        let began = {
            let typed = stem(name, &held.ending).to_lowercase();
            row.completed && row.title.to_lowercase().starts_with(&typed)
        };
        let kept = if held.alone {
            String::new()
        } else if name.is_empty() || began {
            held.settled.clone()
        } else {
            name.to_owned()
        };
        if !held.alone
            && let Some(Control::Save { settled, .. }) = form.controls.get_mut(at)
        {
            kept.clone_into(settled);
        }
        form.open = None;
        form.typed = false;
        form.offered = false;
        form.refusal = None;
        let text = path::save_join(&written(&row.folder), &kept);
        self.save_put(at, text, cx);
        cx.notify();
    }

    /// The Save field `at` lost the keyboard (`on_blur`): its popover
    /// folded, but to its own button (which unfolds and folds it), and a
    /// folder held with no name given the one put there.
    pub(super) fn save_left(&mut self, at: usize, window: &mut Window, cx: &mut Context<Self>) {
        let Some(form) = self.form.as_mut() else {
            return;
        };
        let to_button = matches!(
            form.controls.get(at),
            Some(Control::Save { focus, .. }) if focus.is_focused(window)
        );
        if !to_button && form.open == Some(at) {
            form.open = None;
            form.typed = false;
            form.offered = false;
        }
        let Some(held) = form.held(at, cx) else {
            return;
        };
        if held.alone || held.settled.is_empty() || held.text.is_empty() {
            cx.notify();
            return;
        }
        let (dir, name) = split(&held.text);
        if (name.is_empty() && !dir.is_empty())
            || form.read.names_a_folder(&held.text) == Some(true)
        {
            let text = path::save_join(&held.text, &held.settled);
            self.save_put(at, text, cx);
        }
        cx.notify();
    }

    /// Its button pressed: the popover unfolded or folded, the keyboard
    /// back in its text, where the arrows and Enter act on the list.
    pub(super) fn save_unfold(&mut self, at: usize, window: &mut Window, cx: &mut Context<Self>) {
        self.form_unfold(at, cx);
        if let Some(form) = self.form.as_mut() {
            form.offered = form.open == Some(at);
        }
        if let Some(form) = self.form.as_ref()
            && let Some(Control::Save { input, .. }) = form.controls.get(at)
        {
            use gpui::Focusable as _;
            let focus = input.read(cx).focus_handle(cx);
            window.focus(&focus, cx);
        }
    }

    /// Tab (`ui.form_complete`), as the address field's: the folder
    /// highlighted in the Save field's list; else the start the folders
    /// completing what was typed share, filled in; else the one there is,
    /// put in place of the one typed; else the first highlighted, which
    /// the next Tab takes.
    pub(crate) fn form_complete(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(form) = self.form.as_ref() else {
            return;
        };
        let Some(at) = form.save_focused(window, cx) else {
            return;
        };
        let rows = form.save_rows(at, cx);
        if let Some(row) = form.highlighted(at).and_then(|row| rows.get(row)) {
            self.save_choose(at, row, cx);
            return;
        }
        let Some(held) = form.held(at, cx) else {
            return;
        };
        let shared = form
            .read
            .completions(&held.text, &held.ending)
            .and_then(|(_, shared)| shared);
        if let Some(shared) = shared {
            // The start they share in place of what was typed, the rest
            // (the ending) kept, the caret after it.
            let (dir, name) = split(&held.text);
            let rest = &name[stem(name, &held.ending).len()..];
            let before = path::save_join(dir, &shared);
            let caret = before.len();
            let text = format!("{before}{rest}");
            if let Some(Control::Save { input, .. }) = form.controls.get(at) {
                let input = input.clone();
                input.update(cx, |input, cx| {
                    input.replace_all(&text, cx);
                    input.select_range(caret..caret, cx);
                });
            }
            return;
        }
        let completed: Vec<&Row> = rows.iter().filter(|row| row.completed).collect();
        match completed.as_slice() {
            [] => {}
            [one] => self.save_choose(at, one, cx),
            _ => {
                if let Some(form) = self.form.as_mut() {
                    form.typed = false;
                    form.picker = crate::modal::Picker::default();
                    form.list_scroll.scroll_to_item(0);
                    cx.notify();
                }
            }
        }
    }

    /// Alt+Up (`ui.form_up`) in a Save field: the folder above the one it
    /// holds, the name kept.
    pub(crate) fn form_up(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(form) = self.form.as_mut() else {
            return;
        };
        let Some(at) = form.save_focused(window, cx) else {
            return;
        };
        let Some(held) = form.held(at, cx) else {
            return;
        };
        let (_, name) = split(&held.text);
        // A folder alone is the whole text.
        let folder = if held.alone {
            folder_of(&format!("{}{}", held.text, std::path::MAIN_SEPARATOR))
        } else {
            folder_of(&held.text)
        };
        let Some(above) = folder.as_deref().and_then(Path::parent) else {
            return;
        };
        let kept = if held.alone { "" } else { name };
        let text = path::save_join(&written(above), kept);
        form.open = None;
        form.typed = false;
        self.save_put(at, text, cx);
        cx.notify();
    }

    /// Read the folder the Save field `at`'s text names, on a worker in
    /// its lane, once it answered: what completes it and what it holds
    /// already.
    pub(super) fn save_read(&mut self, at: usize, cx: &mut Context<Self>) {
        let Some(form) = self.form.as_mut() else {
            return;
        };
        let Some(Control::Save { input, .. }) = form.controls.get(at) else {
            return;
        };
        let Some(folder) = folder_of(input.read(cx).text()) else {
            return;
        };
        if !form.read.ask(&folder) {
            return;
        }
        let lane = skwir_core::fs::reach::lane(&folder);
        let app = Arc::clone(&self.app);
        form.reading.ask(
            &self.app,
            lane,
            self.app.lifetime(),
            Priority::Visible,
            "form-complete",
            move |stop| Known::read(&app, folder, stop),
        );
    }
}

/// The popover of the Save field `at`: its `rows` under it, under their
/// headings, over what follows (deferred, after the form's panel),
/// scrolling past `POPOVER`, the highlighted one kept in view
/// (`FormState::list_scroll`); a click chooses one, a press elsewhere in
/// the form folds it.
pub(super) fn popover(
    shell: &Shell,
    state: &FormState,
    at: usize,
    rows: &[Row],
    theme: &Theme,
    cx: &mut Context<Shell>,
) -> AnyElement {
    use gpui::Focusable as _;
    let chosen_row = state.highlighted(at);
    let field_focus = match state.controls.get(at) {
        Some(Control::Save { input, .. }) => input.read(cx).focus_handle(cx),
        _ => state.cancel_focus.clone(),
    };
    let mut list = modal::options("form-folders", "Folders", rows.len(), shell.a11y)
        .flex()
        .flex_col()
        .py_1()
        .max_h(sized(POPOVER))
        .overflow_y_scroll()
        .track_scroll(&state.list_scroll);
    let mut section: Option<&str> = None;
    for (row, folder) in rows.iter().enumerate() {
        if !folder.section.is_empty() && section != Some(folder.section.as_str()) {
            section = Some(&folder.section);
            list = list.child(
                div()
                    .px_2()
                    .pt_1()
                    .text_small()
                    .text_color(theme.text_muted)
                    .child(folder.section.clone()),
            );
        }
        let highlighted = chosen_row == Some(row);
        let named = shell.a11y.then(|| SharedString::from(folder.title.clone()));
        let item = div()
            .id(("form-folder", row))
            .px_2()
            .py_1()
            .mx_1()
            .rounded_sm()
            .flex()
            .flex_col()
            .cursor_pointer()
            .when(highlighted, |d| d.chosen(theme))
            .when(!highlighted, |d| d.hover(|s| s.bg(theme.hover)))
            .child(div().text_body().child(folder.title.clone()))
            .when(!folder.detail.is_empty(), |d| {
                d.child(
                    div()
                        .text_small()
                        .when(!highlighted, |d| d.text_color(theme.text_muted))
                        .truncate()
                        .child(folder.detail.clone()),
                )
            })
            .on_click(cx.listener(move |this, _, window, cx| {
                this.form_choose(Some((at, row)), window, cx);
            }));
        list = list.child(modal::option(item, named, row, highlighted, &field_focus));
    }
    gpui::deferred(
        div()
            .absolute()
            .top_full()
            .left_0()
            .right_0()
            .mt_1()
            .rounded_md()
            .border_1()
            .border_color(theme.border)
            .bg(theme.bg)
            .shadow_md()
            .occlude()
            // A press in it is no press outside it (`form_fold`).
            .on_mouse_down(gpui::MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .child(list)
            // Over its right edge, shown while it has more than fits.
            .child(crate::elements::scrollbar::vertical(
                "form-folders-scrollbar",
                crate::elements::scrollbar::Scrolls::Offset(state.list_scroll.clone()),
            )),
    )
    // Over the form's panel, a modal of priority 3.
    .with_priority(4)
    .into_any_element()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A name's stem is what is selected, its ending kept; a name typed
    /// without the ending is saved with it.
    #[test]
    fn the_name_but_its_ending_is_selected() {
        let text = "~/l'été/ma liste.pdf";
        let range = stem_range(text, ".pdf");
        assert_eq!(&text[range], "ma liste");
        assert_eq!(
            &"~/x/Notes.PDF"[stem_range("~/x/Notes.PDF", ".pdf")],
            "Notes"
        );
        assert_eq!(&"~/x/.pdf"[stem_range("~/x/.pdf", ".pdf")], ".pdf");
        assert_eq!(stem_range("~/x/", ".pdf"), 4..4);
        assert_eq!(ended("ma liste", ".pdf"), "ma liste.pdf");
        assert_eq!(ended("a.PDF", ".pdf"), "a.PDF");
        assert_eq!(ended("notes.md", ".pdf"), "notes.md.pdf");
    }

    /// A folder alone takes the name put there; a whole path stays as
    /// typed; folders are written from `~`, ending with a separator.
    #[test]
    fn a_folder_alone_takes_the_name_put_there() {
        assert_eq!(
            FormState::save_value("~/Documents/", "notes.pdf"),
            "~/Documents/notes.pdf"
        );
        assert_eq!(
            FormState::save_value(" ~/a/b.pdf ", "notes.pdf"),
            "~/a/b.pdf"
        );
        assert_eq!(FormState::save_value("", "notes.pdf"), "notes.pdf");
        // `~` alone is a folder: the name goes in it, not after a tilde.
        assert_eq!(
            FormState::save_value("~", "notes.pdf"),
            format!("~{}notes.pdf", std::path::MAIN_SEPARATOR)
        );
        assert_eq!(FormState::save_value("~", ""), "~", "a folder alone stays");
        assert_eq!(folder_of("letters/x.pdf"), None, "relative");
        assert_eq!(folder_of("x.pdf"), None);
        assert_eq!(
            folder_of("/example/a b/x.pdf"),
            Some(PathBuf::from("/example/a b/"))
        );
        if let Some(home) = skwir_core::os_path::home() {
            assert_eq!(folder_of("~/x.pdf"), Some(home.clone()));
            assert_eq!(written(&home.join("l'été")), "~/l'été/");
        }
        assert_eq!(written(Path::new("/")), "/");
    }

    #[cfg(unix)]
    fn known(folder: &str, names: &[(&str, bool)]) -> Known {
        let holder = complete::Folder {
            path: PathBuf::from(folder),
            names: names
                .iter()
                .map(|(name, dir)| complete::Name::new((*name).into(), *dir, false))
                .collect(),
        };
        answer(
            folder,
            complete::Resolved {
                folders: vec![Arc::new(holder)],
                ..complete::Resolved::default()
            },
        )
    }

    #[cfg(unix)]
    fn answer(folder: &str, resolved: complete::Resolved) -> Known {
        Known::of(PathBuf::from(folder), resolved)
    }

    #[cfg(unix)]
    fn held(text: &str) -> Held {
        Held {
            ending: ".pdf".to_owned(),
            text: text.to_owned(),
            settled: "notes.pdf".to_owned(),
            alone: false,
        }
    }

    /// A folder left before it answered was never read: it is asked
    /// again when it is wanted again, and only then.
    #[cfg(unix)]
    #[test]
    fn a_folder_left_before_it_answered_is_read_again() {
        let (a, b) = (Path::new("/example/a"), Path::new("/example/a/b"));
        let mut reads = Reads::default();
        assert!(reads.ask(a));
        assert!(!reads.ask(a), "under way");
        assert!(reads.ask(b), "the one typed next");
        assert!(reads.ask(a), "back to the first, whose read was let go of");
        reads.answered(known("/example/a", &[("b", true)]));
        assert!(!reads.ask(a), "it answered");
        assert_eq!(reads.folder_is("/example/a/x.pdf"), Some(true));
    }

    /// A read that could not tell neither holds the form back nor is
    /// kept; one that says the folder is not there does.
    #[cfg(unix)]
    #[test]
    fn a_read_that_did_not_answer_says_nothing_and_is_asked_again() {
        let shut = complete::Resolved {
            unread: true,
            ..complete::Resolved::default()
        };
        let folder = Path::new("/example/shut");
        let text = "/example/shut/x.pdf";
        let mut reads = Reads::default();
        assert!(reads.ask(folder));
        reads.answered(answer("/example/shut", shut));
        assert_eq!(reads.folder_is(text), None);
        assert_eq!(reads.names_a_folder(text), None);
        assert!(reads.note(&held(text), "", "").is_none());
        assert!(reads.ask(folder), "not kept");

        reads.answered(answer("/example/shut", complete::Resolved::default()));
        let note = reads.note(&held(text), "", "").expect("said");
        assert!(note.blocks);
        assert_eq!(note.text, "There is no folder “/example/shut”");
        assert!(!reads.ask(folder), "kept");
    }

    /// What Tab fills in is what the folders listed share: a file that
    /// starts alike takes no part.
    #[cfg(unix)]
    #[test]
    fn tab_fills_in_what_the_folders_share() {
        let completing = |names: &[(&str, bool)], text: &str| {
            let mut reads = Reads::default();
            reads.answered(known("/example/a", names));
            let (rows, shared) = reads.completions(text, ".pdf").expect("known");
            let titles: Vec<String> = rows.iter().map(|row| row.title.clone()).collect();
            (titles, shared)
        };
        assert_eq!(
            completing(&[("projects", true), ("pro.txt", false)], "/example/a/pr"),
            (vec!["projects".to_owned()], None),
            "one folder: Tab takes it"
        );
        assert_eq!(
            completing(
                &[
                    ("report-2024", true),
                    ("report-2025", true),
                    ("reply.txt", false)
                ],
                "/example/a/re"
            ),
            (
                vec!["report-2024".to_owned(), "report-2025".to_owned()],
                Some("report-202".to_owned())
            )
        );
    }

    /// The words under the field and the rows ask what the read holds,
    /// not its names one by one, and rows are worked out once for a text
    /// and what was read.
    #[cfg(unix)]
    #[test]
    fn what_completes_a_text_is_worked_out_once() {
        let mut reads = Reads::default();
        reads.answered(known(
            "/example/a",
            &[("drafts", true), ("notes.pdf", false)],
        ));
        assert_eq!(reads.names_a_folder("/example/a/drafts"), Some(true));
        assert_eq!(reads.names_a_folder("/example/a/notes.pdf"), Some(false));
        let note = reads.note(&held("/example/a/notes"), "", "").expect("said");
        assert!(note.warns && !note.blocks, "there already");

        let text = "/example/a/dr";
        let (first, _) = reads.completions(text, ".pdf").unwrap();
        let (again, _) = reads.completions(text, ".pdf").unwrap();
        assert!(Rc::ptr_eq(&first, &again), "kept while nothing moved");
        let (other, _) = reads.completions("/example/a/d", ".pdf").unwrap();
        assert!(!Rc::ptr_eq(&first, &other), "another text");
        reads.answered(known("/example/a", &[("dry", true)]));
        let (read_again, _) = reads.completions(text, ".pdf").unwrap();
        assert_eq!(read_again[0].title, "dry", "what was read since");
    }
}
