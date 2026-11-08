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
//!   held; else the field's help.
use crate::theme::Chosen;
use std::collections::HashMap;
use std::ops::Range;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use gpui::{AnyElement, Context, SharedString, Window, div, prelude::*};
use skwir_core::form::{Field, Kind};

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

/// The folders read for completion, by path: `None` while being read.
pub(super) type Read = HashMap<PathBuf, Option<Arc<complete::Resolved>>>;

/// The folders kept read for one form, at most.
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

/// `text` split where completion splits it (`complete::split`): its
/// folder part, with its last separator, and its name; `~` alone is the
/// home folder.
fn split(text: &str) -> (&str, &str) {
    if text == "~" {
        (text, "")
    } else {
        complete::split(text)
    }
}

/// The folder `text`'s folder part names, `~` the home folder: `None`
/// without one, or for a relative one.
fn folder_of(text: &str) -> Option<PathBuf> {
    let (dir, _) = split(text);
    if dir.is_empty() {
        return None;
    }
    complete::resolve(dir, None, skwir_core::os_path::home().as_deref())
        .filter(|path| path.is_absolute())
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
            format!("{text}{settled}")
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

    /// Whether `text`'s name, after its folder, is one of that folder's
    /// own folders: `None` until the folder is read.
    fn names_a_folder(&self, text: &str) -> Option<bool> {
        let (_, name) = split(text);
        if name.is_empty() {
            return Some(false);
        }
        let folder = folder_of(text)?;
        let read = self.read.get(&folder)?.as_ref()?;
        let holder = read.folders.first().filter(|f| f.path == folder)?;
        Some(holder.names.iter().any(|n| n.dir && n.text == name))
    }

    /// Whether the folder `text`'s folder part names is one: `None` until
    /// it is read.
    fn folder_is(&self, text: &str) -> Option<bool> {
        let folder = folder_of(text)?;
        let read = self.read.get(&folder)?.as_ref()?;
        Some(
            !read.abbreviated
                && read
                    .folders
                    .first()
                    .is_some_and(|holder| holder.path == folder),
        )
    }

    /// The rows of the Save field `at`'s list: the asker's folders when
    /// asked for (`offered`), else what completes the folder typed once
    /// its text was edited, else none.
    pub(super) fn save_rows(&self, at: usize, cx: &gpui::App) -> Vec<Row> {
        let (Some(field), Some(Control::Save { input, put, .. })) =
            (self.form.fields.get(at), self.controls.get(at))
        else {
            return Vec::new();
        };
        let Kind::Save {
            folders, ending, ..
        } = &field.kind
        else {
            return Vec::new();
        };
        let text = input.read(cx).text();
        if !self.offered {
            return if text == put.as_str() {
                Vec::new()
            } else {
                self.completions(text, ending)
                    .map(|(rows, _)| rows)
                    .unwrap_or_default()
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

    /// The folders within the folder `text` names that its name typed
    /// begins (its ending aside), best first, and the start they share
    /// beyond what was typed; `None` until the folder is read.
    fn completions(&self, text: &str, ending: &str) -> Option<(Vec<Row>, Option<String>)> {
        let folder = folder_of(text)?;
        let resolved = self.read.get(&folder)?.as_ref()?;
        let typed = stem(split(text).1, ending);
        let (found, shared) = complete::rank_at_most(&resolved.folders, typed, false, LISTED);
        // Where each is said only when they come from several folders (a
        // folder part abbreviated): they are all in the one typed else.
        let several = resolved.folders.len() > 1;
        let rows = found
            .into_iter()
            .filter_map(|m| {
                let holder = &resolved.folders[m.folder];
                let name = &holder.names[m.index];
                name.dir.then(|| Row {
                    title: name.text.clone(),
                    detail: if several {
                        skwir_core::os_path::from_home(&holder.path)
                    } else {
                        String::new()
                    },
                    section: String::new(),
                    folder: holder.path.join(&name.exact),
                    completed: true,
                })
            })
            .collect();
        Some((rows, shared))
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
        let held = self.held(at, cx)?;
        let text = held.text.as_str();
        let (dir, name) = split(text);
        if text.is_empty() || (dir.is_empty() && !held.alone) {
            return (!beside.is_empty()).then(|| Note::says(beside.clone()));
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
                Some(false) if !name.is_empty() => missing(text),
                _ => (!alone.is_empty()).then(|| Note::says(alone.clone())),
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
        let folder = folder_of(text)?;
        let read = self.read.get(&folder)?.as_ref()?;
        let taken = read
            .folders
            .first()
            .is_some_and(|holder| holder.names.iter().any(|n| n.text == full));
        if taken {
            return Some(Note::warns(format!(
                "“{full}” is there already: saving asks whether to replace it"
            )));
        }
        None
    }
}

impl Shell {
    /// The Save fields as a form opens: each holding the name its `names`
    /// give where its value is a folder alone, the name but its ending
    /// selected, its folder read.
    pub(super) fn save_open(&mut self, window: &mut Window, cx: &mut Context<Self>) {
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
            self.save_read(at, window, cx);
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
            let followed = format!("{dir}{name}");
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
    fn save_put(&mut self, at: usize, text: String, window: &mut Window, cx: &mut Context<Self>) {
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
        self.save_read(at, window, cx);
    }

    /// Put `row`'s folder in place of the one the Save field `at` holds,
    /// keeping its name (see the module's docs); its list folded.
    pub(super) fn save_choose(
        &mut self,
        at: usize,
        row: &Row,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
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
        let text = format!("{}{kept}", written(&row.folder));
        self.save_put(at, text, window, cx);
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
        let named = if name.is_empty() && !dir.is_empty() {
            Some(format!("{}{}", held.text, held.settled))
        } else if form.names_a_folder(&held.text) == Some(true) {
            Some(format!(
                "{}{}{}",
                held.text,
                std::path::MAIN_SEPARATOR,
                held.settled
            ))
        } else {
            None
        };
        if let Some(text) = named {
            self.save_put(at, text, window, cx);
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
            self.save_choose(at, row, window, cx);
            return;
        }
        let Some(held) = form.held(at, cx) else {
            return;
        };
        let shared = form
            .completions(&held.text, &held.ending)
            .and_then(|(_, shared)| shared);
        if let Some(shared) = shared {
            // The start they share in place of what was typed, the rest
            // (the ending) kept, the caret after it.
            let (dir, name) = split(&held.text);
            let rest = &name[stem(name, &held.ending).len()..];
            let text = format!("{dir}{shared}{rest}");
            let caret = dir.len() + shared.len();
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
            [one] => self.save_choose(at, one, window, cx),
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
        let text = format!("{}{kept}", written(above));
        form.open = None;
        form.typed = false;
        self.save_put(at, text, window, cx);
        cx.notify();
    }

    /// Read the folder the Save field `at`'s text names, once, on a
    /// worker in its lane: what completes it and what it holds already.
    pub(super) fn save_read(&mut self, at: usize, window: &mut Window, cx: &mut Context<Self>) {
        let Some(form) = self.form.as_mut() else {
            return;
        };
        let Some(Control::Save { input, .. }) = form.controls.get(at) else {
            return;
        };
        let Some(folder) = folder_of(input.read(cx).text()) else {
            return;
        };
        if form.read.contains_key(&folder) {
            return;
        }
        if form.read.len() >= KEPT {
            form.read.clear();
        }
        form.read.insert(folder.clone(), None);
        // One folder read at a time: the one typed before is let go of.
        let cancel = self.app.lifetime().child();
        if let Some(before) = form.reading.replace(cancel.clone()) {
            before.cancel();
        }
        let (id, app, asked, stop) = (
            form.id,
            Arc::clone(&self.app),
            folder.clone(),
            cancel.clone(),
        );
        self.off_thread_until(
            skwir_core::fs::reach::lane(&folder),
            "form-complete",
            cancel,
            move || complete::folders(&app, &asked, false, &stop),
            move |shell, resolved, _, cx| {
                let Some(form) = shell.form.as_mut().filter(|form| form.id == id) else {
                    return;
                };
                if let Some(slot) = form.read.get_mut(&folder) {
                    *slot = Some(Arc::new(resolved));
                }
                cx.notify();
            },
            window,
            cx,
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
}
