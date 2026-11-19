//! Bulk rename dialog: template, regex, replacement, case, start; live
//! preview from `rename_bulk.preview`, where the regex matched in each old
//! name and what it put in the new one tinted; Enter or Rename applies
//! `rename_bulk`. The template, the regex and the replacement each say
//! what they take (ADR-137): the plugin's `rename_bulk.reference`, the
//! template's asked with each preview, the regex's and the replacement's
//! once for the first name (`kept_for`), opened from the `?` inside the
//! field at its end or F1 in it, and what goes on with a part begun
//! offered under the field as it is typed (`reference.rs`). It opens on
//! `{full}`, which leaves every name as it is. The case is a row of its
//! values side by side, a dropdown where they do not fit
//! (`case_control`).

use std::ops::Range;
use std::rc::Rc;

use crate::sizes::TextSizes;
use crate::sizes::sized;
use gpui::{AnyElement, Context, Entity, Subscription, Window, div, prelude::*};
use skwir_core::EntryId;
use skwir_core::command::{Invocation, Outcome};
use skwir_core::reference::Reference;
use skwir_core::words;
use skwir_core::words::Words;

use crate::reference::FieldHelp;
use skwir_core::workspace::PaneId;

use crate::controls::{self, ButtonStyle};
use crate::elements::clip::clipped_marked;
use crate::elements::popover::PopoverAnchor;
use crate::input::{InputEvent, TextInput};
use crate::keymap;
use crate::modal::{self, FocusReturn};
use crate::shell::Shell;
use crate::theme::Theme;

/// The cases a rename gives names: the value the plugin knows, labelled
/// by `case_label`. The first asks for none.
pub const CASES: [&str; 5] = ["none", "lower", "upper", "title", "sentence"];

/// The label of the case at `at` of `CASES`, written as the case leaves a
/// name.
fn case_label(at: usize) -> Words {
    match at {
        0 => words!(
            renamebulk_case_unchanged = "Unchanged",
            note = "A case the bulk rename gives names: it leaves them as they are."
        ),
        1 => words!(
            renamebulk_case_lower = "lowercase",
            note = "A case the bulk rename gives names, written as it leaves them: every \
                    letter small."
        ),
        2 => words!(
            renamebulk_case_upper = "UPPERCASE",
            note = "A case the bulk rename gives names, written as it leaves them: every \
                    letter a capital."
        ),
        3 => words!(
            renamebulk_case_title = "Title Case",
            note = "A case the bulk rename gives names, written as it leaves them: each word \
                    begins with a capital."
        ),
        _ => words!(
            renamebulk_case_sentence = "Sentence case",
            note = "A case the bulk rename gives names, written as it leaves them: only the \
                    first word begins with a capital."
        ),
    }
}

/// Each field, by the key the plugin knows it by; `field_words` says its
/// label and what it shows while empty.
const FIELDS: [&str; 4] = ["template", "regex", "replacement", "start"];

/// The label of the field at `at` of `FIELDS`, and what it shows while
/// empty.
fn field_words(at: usize) -> (Words, Words) {
    match at {
        0 => (
            words!(
                renamebulk_template = "Template",
                note = "The bulk rename's field of what each new name is made of."
            ),
            // A template as it is typed.
            Words::as_written("{name}-{n:02}{.ext}"),
        ),
        1 => (
            words!(
                renamebulk_regex = "Regex",
                note = "The bulk rename's field of a regular expression matched in each name."
            ),
            words!(
                renamebulk_regex_placeholder = "Optional: a pattern over the whole name",
                note = "In the bulk rename's empty Regex field."
            ),
        ),
        2 => (
            words!(
                renamebulk_replacement = "Replacement",
                note = "The bulk rename's field of what each match of the regex becomes."
            ),
            words!(
                renamebulk_replacement_placeholder = "Optional: what each match becomes, $1 \
                                                      for its first group",
                note = "In the bulk rename's empty Replacement field; $1 is written as it is \
                        typed."
            ),
        ),
        _ => (
            words!(
                renamebulk_start = "Start",
                note = "The bulk rename's field of the number the names' counter starts at."
            ),
            Words::as_written("1"),
        ),
    }
}

/// How many of `FIELDS`, from the first, say what they take.
const SAID: usize = 3;

/// A rename the preview shows: the old name and where the regex matched
/// in it, the new one and what the regex put there (byte ranges).
pub struct Renamed {
    from: String,
    to: String,
    matched: Vec<Range<usize>>,
    put: Vec<Range<usize>>,
}

/// Byte ranges as the plugin says them (`[[start, end], …]`).
fn ranges(said: Option<&serde_json::Value>) -> Vec<Range<usize>> {
    said.and_then(serde_json::Value::as_array)
        .map_or(&[][..], Vec::as_slice)
        .iter()
        .filter_map(|range| {
            let [start, end] = serde_json::from_value::<[usize; 2]>(range.clone()).ok()?;
            (start <= end).then_some(start..end)
        })
        .collect()
}

/// The renames the preview shows at most.
const SHOWN: usize = 40;

pub struct BulkState {
    /// The pane it was opened for, and what was selected there then: it
    /// renames those, whatever pane is active or is selected later.
    pane: PaneId,
    selected: Vec<EntryId>,
    pub inputs: Vec<Entity<TextInput>>,
    /// The case chosen (`CASES`), the keyboard of its row, and, where the
    /// row gives way to a dropdown, whether its list is open and where it
    /// hangs from.
    pub case: usize,
    case_focus: gpui::FocusHandle,
    case_open: bool,
    case_anchor: PopoverAnchor,
    pub focused: usize,
    /// The first names the preview shows (`SHOWN`), and how many renames
    /// there are in all.
    pub preview: Vec<Renamed>,
    /// What the template, the regex and the replacement take, as the
    /// plugin said it with the latest preview (`reference.rs`).
    helps: [FieldHelp; SAID],
    /// The first name the regex's and the replacement's references were
    /// said for: they read it alone (what the recipes make of it), so
    /// they are asked again only when it changes, never at each key. Kept
    /// with the dialog, gone with it.
    kept_for: Option<String>,
    renamed: usize,
    pub error: Option<Words>,
    /// Advances with each change; only the latest preview is shown, and
    /// the one asked before is cancelled (`pending`): not worked out if
    /// it has not begun, stopped where it is if it has.
    asked: u64,
    pending: Option<skwir_core::CancelToken>,
    /// The preview shown is the answer to the latest change.
    current: bool,
    count: usize,
    back: FocusReturn,
    cancel_focus: gpui::FocusHandle,
    apply_focus: gpui::FocusHandle,
    /// Where the renames listed are scrolled to.
    scroll: gpui::ScrollHandle,
    _subs: Vec<Subscription>,
}

impl BulkState {
    /// Only what the preview showed applies, and only if it showed no
    /// error and a name that changes: while none does, Rename is inert and
    /// the preview says so.
    fn applies(&self) -> bool {
        self.current && self.error.is_none() && self.renamed > 0
    }

    /// The preview is the latest change's, and no name changes.
    fn changes_nothing(&self) -> bool {
        self.current && self.error.is_none() && self.renamed == 0
    }

    /// What Tab walks: its fields, the case, then Cancel and Rename.
    pub(crate) fn walk(&self, cx: &gpui::App) -> Vec<gpui::FocusHandle> {
        use gpui::Focusable as _;
        self.inputs
            .iter()
            .map(|input| input.read(cx).focus_handle(cx))
            .chain([
                self.case_focus.clone(),
                self.cancel_focus.clone(),
                self.apply_focus.clone(),
            ])
            .collect()
    }
}

/// The fields of the shell's dialogs that say what they take (ADR-137):
/// a question's, the bulk rename's, each known by its input, so a question
/// open over the bulk rename never answers for its fields, whatever its
/// field is named.
impl crate::reference::Helps for Shell {
    fn help_of(&mut self, field: gpui::EntityId) -> Option<(&mut FieldHelp, Entity<TextInput>)> {
        if self.prompt.as_ref().is_some_and(|p| p.helps(field)) {
            return self.prompt.as_mut().and_then(|p| p.help_of(field));
        }
        let b = self.bulk.as_mut()?;
        let at = b
            .inputs
            .iter()
            .position(|input| input.entity_id() == field)?;
        let input = b.inputs[at].clone();
        Some((b.helps.get_mut(at)?, input))
    }

    fn field_of(&self, field: gpui::EntityId, key: &str) -> Option<Entity<TextInput>> {
        // A question has one field.
        if let Some(p) = self.prompt.as_ref().filter(|p| p.helps(field)) {
            return p.names(key).then(|| p.input.clone());
        }
        let b = self.bulk.as_ref()?;
        if !b.inputs.iter().any(|input| input.entity_id() == field) {
            return None;
        }
        let at = FIELDS.iter().position(|named| *named == key)?;
        b.inputs.get(at).cloned()
    }
}

impl Shell {
    fn bulk_spec(&self, cx: &Context<Self>) -> serde_json::Value {
        let Some(b) = &self.bulk else {
            return serde_json::json!({});
        };
        let texts: [String; 4] = std::array::from_fn(|i| b.inputs[i].read(cx).text().to_string());
        spec_of(&texts, b.case, b.pane)
    }

    /// Whether the pane it was opened for still selects what it did: the
    /// dialog renames nothing else (a selection changed meanwhile, by an
    /// agent's cursor or a listing renamed under it).
    fn bulk_still_selected(&self) -> bool {
        let Some(b) = &self.bulk else {
            return false;
        };
        let ws = self.app.workspace.read();
        ws.pane(b.pane).is_some_and(|p| {
            p.selection.len() == b.selected.len()
                && b.selected.iter().all(|id| p.selection.contains(*id))
        })
    }

    pub fn open_bulk(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let count = self.selection_ctx().count;
        if count == 0 {
            self.set_status(crate::shell::nothing_selected(), false, cx);
            return;
        }
        let back = FocusReturn::capture(window, cx);
        let pane = self.pane;
        let selected: Vec<EntryId> = {
            let ws = self.app.workspace.read();
            ws.pane(pane)
                .map(|p| p.selection.iter().collect())
                .unwrap_or_default()
        };
        let mut inputs = Vec::new();
        let mut subs = Vec::new();
        for i in 0..FIELDS.len() {
            let (label, placeholder) = field_words(i);
            let input = cx.new(|cx| {
                let mut input = TextInput::new(placeholder, cx);
                input.set_label(label);
                input
            });
            if i == 0 {
                // The whole name: renaming nothing until it is typed over.
                input.update(cx, |t, cx| {
                    t.set_text("{full}", cx);
                    t.select_all_now(cx);
                });
            }
            subs.push(cx.subscribe_in(
                &input,
                window,
                move |this, input, ev: &InputEvent, window, cx| match ev {
                    // What the field lists takes Enter the arrows chose
                    // a row for, and Escape first (ADR-137).
                    InputEvent::Submit => {
                        if !crate::reference::on_submit(this, input.entity_id(), window, cx) {
                            this.bulk_apply(window, cx);
                        }
                    }
                    InputEvent::Cancel => {
                        if !crate::reference::on_cancel(this, input.entity_id(), window, cx) {
                            this.close_bulk(window, cx);
                        }
                    }
                    InputEvent::Changed => this.bulk_refresh(window, cx),
                },
            ));
            inputs.push(input);
        }
        inputs[0].update(cx, |i, cx| i.focus(window, cx));
        self.bulk = Some(BulkState {
            pane,
            selected,
            inputs,
            case: 0,
            case_focus: cx.focus_handle(),
            case_open: false,
            case_anchor: PopoverAnchor::default(),
            focused: 0,
            preview: Vec::new(),
            helps: std::array::from_fn(|i| FieldHelp::new(FIELDS[i], None)),
            kept_for: None,
            renamed: 0,
            error: None,
            asked: 0,
            pending: None,
            current: false,
            count,
            back,
            cancel_focus: cx.focus_handle(),
            apply_focus: cx.focus_handle(),
            scroll: gpui::ScrollHandle::new(),
            _subs: subs,
        });
        self.bulk_refresh(window, cx);
        cx.notify();
    }

    pub fn close_bulk(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(bulk) = self.bulk.take() {
            if let Some(pending) = &bulk.pending {
                pending.cancel();
            }
            bulk.back.restore(window, &self.ui().focus, cx);
        }
        cx.notify();
    }

    /// Tab and Shift+Tab: the next or the previous of its fields, Cancel
    /// and Rename, round their ends, from what has the keyboard (a click
    /// may have moved it); a field reached has its text selected.
    pub fn bulk_focus_next(&mut self, delta: isize, window: &mut Window, cx: &mut Context<Self>) {
        let Some(b) = &mut self.bulk else { return };
        b.case_open = false;
        let walk = b.walk(cx);
        if !walk.iter().any(|h| h.is_focused(window)) {
            b.inputs[b.focused].update(cx, |i, cx| i.focus(window, cx));
        }
        modal::tab_among(&walk.iter().collect::<Vec<_>>(), delta, window, cx);
        if let Some(at) = b.inputs.iter().position(|i| i.read(cx).is_focused(window)) {
            b.focused = at;
            b.inputs[at].update(cx, TextInput::select_all_now);
        }
        cx.notify();
    }

    /// `ui.rename_bulk_case`: the case `delta` steps from the one chosen,
    /// stopping at the ends (the arrows on its row); without one, the next,
    /// round the last (Ctrl+K from any of its fields, Space on the row).
    pub fn bulk_case(&mut self, delta: Option<isize>, window: &mut Window, cx: &mut Context<Self>) {
        let Some(b) = &self.bulk else { return };
        let next = case_stepped(b.case, delta);
        self.bulk_choose_case(next, window, cx);
    }

    /// Give names case `case`: the preview is asked again if it changed.
    fn bulk_choose_case(&mut self, case: usize, window: &mut Window, cx: &mut Context<Self>) {
        let Some(b) = &mut self.bulk else { return };
        if b.case == case {
            cx.notify();
            return;
        }
        b.case = case;
        self.bulk_refresh(window, cx);
    }

    /// Open or close the case's list, where the row is a dropdown.
    fn bulk_case_list(&mut self, open: bool, cx: &mut Context<Self>) {
        if let Some(b) = &mut self.bulk {
            b.case_open = open;
            cx.notify();
        }
    }

    /// Ask for the preview again after a change, on a worker: the plugin
    /// reads every selected entry and may stat the targets. The one asked
    /// before is cancelled, and says the first `SHOWN` renames alone, with
    /// how many there are.
    fn bulk_refresh(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !self.bulk_still_selected() {
            return self.bulk_selection_changed(cx);
        }
        // The rename as it stands: what the references are asked with,
        // and the preview, which says the first `SHOWN` alone.
        let said = self.bulk_spec(cx);
        let mut spec = said.clone();
        spec["limit"] = SHOWN.into();
        let cancel = self.app.lifetime().child();
        let Some(b) = &mut self.bulk else { return };
        b.asked += 1;
        b.current = false;
        let asked = b.asked;
        let pane = b.pane;
        if let Some(before) = b.pending.replace(cancel.clone()) {
            before.cancel();
        }
        let app = std::sync::Arc::clone(&self.app);
        let kept_for = b.kept_for.clone();
        self.off_thread_until(
            self.pane_lane(pane),
            "rename-bulk-preview",
            cancel,
            move || {
                let (shown, count, error, first) =
                    match app.dispatch(&Invocation::with_args("rename_bulk.preview", &spec)) {
                        Ok(Outcome::Value(v)) => {
                            let shown = v["renames"]
                                .as_array()
                                .map_or(&[][..], Vec::as_slice)
                                .iter()
                                .filter_map(|x| {
                                    Some(Renamed {
                                        from: x.get("from")?.as_str()?.to_owned(),
                                        to: x.get("to")?.as_str()?.to_owned(),
                                        matched: ranges(x.get("matched")),
                                        put: ranges(x.get("put")),
                                    })
                                })
                                .collect();
                            let count = v["count"]
                                .as_u64()
                                .and_then(|n| usize::try_from(n).ok())
                                .unwrap_or_default();
                            let first = v["first"].as_str().map(str::to_owned);
                            (shown, count, None, first)
                        }
                        Ok(_) => (Vec::new(), 0, None, None),
                        Err(e) => (Vec::new(), 0, Some(Words::of_error(e)), None),
                    };
                // What each field takes, given the first item as the rename
                // stands; one the plugin could not say keeps the last. The
                // regex's and the replacement's read the first name alone:
                // asked again only for another one.
                let again = asks_again(first.as_deref(), kept_for.as_deref());
                let references: [Option<Reference>; SAID] = std::array::from_fn(|i| {
                    if i > 0 && !again {
                        return None;
                    }
                    let mut asked = said.clone();
                    asked["field"] = FIELDS[i].into();
                    match app.dispatch(&Invocation::with_args("rename_bulk.reference", &asked)) {
                        Ok(Outcome::Value(v)) => serde_json::from_value(v).ok(),
                        Ok(_) => None,
                        Err(e) => {
                            tracing::warn!("rename_bulk.reference ({}): {e:#}", FIELDS[i]);
                            None
                        }
                    }
                });
                let kept = (again && references[1..].iter().all(Option::is_some))
                    .then_some(first)
                    .flatten();
                (shown, count, error, references, kept)
            },
            move |this, result, _, cx| {
                let (preview, renamed, error, references, kept) = result;
                let Some(b) = this.bulk.as_mut().filter(|b| b.asked == asked) else {
                    return;
                };
                b.pending = None;
                b.preview = preview;
                b.renamed = renamed;
                b.error = error;
                b.current = true;
                for (help, said) in b.helps.iter_mut().zip(references) {
                    if let Some(said) = said {
                        help.reference = Some(Rc::new(said));
                    }
                }
                if kept.is_some() {
                    b.kept_for = kept;
                }
                cx.notify();
            },
            window,
            cx,
        );
        cx.notify();
    }

    fn bulk_apply(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !self.bulk_still_selected() {
            return self.bulk_selection_changed(cx);
        }
        let spec = self.bulk_spec(cx);
        if self.bulk.as_ref().is_some_and(|b| !b.applies()) {
            return;
        }
        self.close_bulk(window, cx);
        self.run("rename_bulk", spec, window, cx);
    }

    /// What it was opened for is no longer selected: said where the
    /// renames were, and nothing applies.
    fn bulk_selection_changed(&mut self, cx: &mut Context<Self>) {
        let Some(b) = &mut self.bulk else { return };
        if let Some(pending) = b.pending.take() {
            pending.cancel();
        }
        b.asked += 1;
        b.preview.clear();
        b.renamed = 0;
        b.error = Some(words!(
            renamebulk_selection_changed = "The selection changed since this opened: open it \
                                            again",
            note = "In the bulk rename, when what it was opened for is no longer selected."
        ));
        b.current = true;
        cx.notify();
    }
}

/// Whether the references that read the first name alone (the regex's,
/// the replacement's) are asked again: for another first name than the
/// one they were said for, or while none was (a preview that failed asks
/// them each time, as before it knew).
fn asks_again(first: Option<&str>, kept_for: Option<&str>) -> bool {
    match (first, kept_for) {
        (Some(first), Some(kept)) => first != kept,
        (_, None) => true,
        (None, Some(_)) => false,
    }
}

/// The case `delta` steps from `case`, stopping at the ends; without one,
/// the next, round the last (`Shell::bulk_case`).
fn case_stepped(case: usize, delta: Option<isize>) -> usize {
    match delta {
        None => (case + 1) % CASES.len(),
        Some(delta) => case.saturating_add_signed(delta).min(CASES.len() - 1),
    }
}

/// The rename the fields ask (`FIELDS`' texts, the case chosen) for the
/// pane it was opened for. The template and the start are read without
/// what is around them; the regex and the replacement as typed, a space
/// being something a pattern matches or a match becomes (a recipe's
/// underscores to spaces). Empty, a field asks nothing.
fn spec_of(texts: &[String; 4], case: usize, pane: PaneId) -> serde_json::Value {
    let [template, regex, replacement, start] = texts;
    let opt = |s: &str| {
        if s.is_empty() {
            serde_json::Value::Null
        } else {
            serde_json::Value::String(s.to_owned())
        }
    };
    let start: u64 = start.trim().parse().unwrap_or(1);
    let case = match CASES.get(case) {
        Some(&value) if case > 0 => serde_json::Value::String(value.into()),
        _ => serde_json::Value::Null,
    };
    serde_json::json!({
        "template": opt(template.trim()),
        "regex": opt(regex),
        "replacement": opt(replacement),
        "case": case,
        "start": start,
        "pane": pane,
    })
}

/// The case names are given: its values side by side, each written as
/// it leaves a name; where they do not fit, a dropdown (`adaptive::Fit`).
/// One stop for Tab, a radio group to a screen reader: the arrows step it
/// (`ui.rename_bulk_case`), Space gives the next case on the row and opens
/// the dropdown's list, Enter renames as from a field. Its key from any
/// field is said in the footer, with the dialog's other keys.
fn case_control(b: &BulkState, theme: &Theme, cx: &mut Context<Shell>) -> AnyElement {
    crate::elements::adaptive::Fit {
        variants: vec![case_segments(b, theme, cx), case_dropdown(b, theme, cx)],
        cuts: false,
        start: true,
    }
    .into_any_element()
}

/// The case's values as segments, the one chosen filled.
fn case_segments(b: &BulkState, theme: &Theme, cx: &mut Context<Shell>) -> AnyElement {
    // Space and Enter reach the focused row as a keyboard click; the
    // pointer's clicks belong to its segments.
    let keyboard = cx.listener(|this, event: &gpui::ClickEvent, window, cx| {
        if let gpui::ClickEvent::Keyboard(key) = event {
            match key.button {
                gpui::KeyboardButton::Space => this.bulk_case(None, window, cx),
                gpui::KeyboardButton::Enter => this.bulk_apply(window, cx),
            }
        }
    });
    let mut frame = controls::segmented("bulk-case", &b.case_focus, theme)
        .aria_label(case_title().say())
        .key_context(keymap::BULK_CASE)
        .on_click(keyboard);
    // Drawn as segments, the dropdown's list has nothing to hang from (the
    // window widened while it was open): it is closed, never left to show
    // again unasked when the row narrows.
    if b.case_open {
        let shell = cx.entity().downgrade();
        frame = frame.child(
            gpui::canvas(
                move |_, _, cx| {
                    cx.defer(move |cx| {
                        shell
                            .update(cx, |shell, cx| shell.bulk_case_list(false, cx))
                            .ok();
                    });
                },
                |_, (), _, _| {},
            )
            .absolute(),
        );
    }
    for at in 0..CASES.len() {
        let focus = b.case_focus.clone();
        let label = case_label(at).say();
        frame = frame.child(
            controls::segment(format!("bulk-case-{at}"), label, at == b.case, theme).on_click(
                cx.listener(move |this, _, window, cx| {
                    window.focus(&focus, cx);
                    this.bulk_choose_case(at, window, cx);
                }),
            ),
        );
    }
    frame.into_any_element()
}

/// The case as a dropdown: its value, and the list of them a press or
/// Space opens, Escape, a press outside or a value taken closes.
fn case_dropdown(b: &BulkState, theme: &Theme, cx: &mut Context<Shell>) -> AnyElement {
    let open = b.case_open;
    let control = controls::dropdown("bulk-case", case_label(b.case).say(), &b.case_focus, theme)
        .aria_label(case_title().say())
        .aria_expanded(open)
        .key_context(keymap::BULK_CASE)
        .child(b.case_anchor.marker())
        .on_click(cx.listener(
            move |this, event: &gpui::ClickEvent, window, cx| match event {
                gpui::ClickEvent::Keyboard(key)
                    if key.button == gpui::KeyboardButton::Enter && !open =>
                {
                    this.bulk_apply(window, cx);
                }
                _ => this.bulk_case_list(!open, cx),
            },
        ))
        .on_key_down(cx.listener(move |this, event: &gpui::KeyDownEvent, _, cx| {
            let key = &event.keystroke;
            if open && key.key == "escape" && !key.modifiers.modified() {
                cx.stop_propagation();
                this.bulk_case_list(false, cx);
            }
        }));
    if !open {
        return control.into_any_element();
    }
    let mut list = controls::dropdown_list("bulk-case-list", theme).on_mouse_down_out(
        b.case_anchor
            .outside(cx.listener(|this, _, _, cx| this.bulk_case_list(false, cx))),
    );
    for at in 0..CASES.len() {
        let focus = b.case_focus.clone();
        list = list.child(
            controls::dropdown_row(format!("bulk-case-row-{at}"), at == b.case, theme)
                .child(case_label(at).say())
                .on_click(cx.listener(move |this, _, window, cx| {
                    window.focus(&focus, cx);
                    this.bulk_case_list(false, cx);
                    this.bulk_choose_case(at, window, cx);
                })),
        );
    }
    // Over the dialog, under the lists of its fields (`reference.rs`).
    let width = theme
        .sizes
        .scale(crate::sizes::Part::Interface)
        .px(CASE_LIST_WIDTH);
    control
        .child(
            b.case_anchor
                .popup_under(CASE_LIST_PRIORITY, width, list.into_any_element()),
        )
        .into_any_element()
}

/// How wide the case's list is at 100 %, and where it is drawn among
/// deferred draws: over the dialog (`modal::modal`'s 4).
const CASE_LIST_WIDTH: f32 = 180.;
const CASE_LIST_PRIORITY: usize = 5;

/// The labels of the fields and of the case at their narrowest and at
/// their widest, at 100 % (`labels::column`): the fields keep the most of
/// the dialog's width.
const LABEL: f32 = 100.;
const LABEL_MOST: f32 = 200.;

pub fn render(shell: &Shell, window: &Window, cx: &mut Context<Shell>) -> Option<AnyElement> {
    let b = shell.bulk.as_ref()?;
    let theme = cx.global::<Theme>().clone();
    // The fields' labels and the case's are as wide as their widest word,
    // the fields beside them in one column.
    let fields: Vec<String> = (0..FIELDS.len()).map(|i| field_words(i).0.say()).collect();
    let case = case_title().say();
    let column = crate::labels::column(
        fields.iter().chain([&case]).map(String::as_str),
        crate::sizes::Text::Body,
        LABEL,
        LABEL_MOST,
        &theme,
        cx,
    );
    let mut form = div().flex().flex_col().gap_2();
    for (i, label) in fields.into_iter().enumerate() {
        // Where the keyboard is, whether Tab or a click put it there.
        let focused = b.inputs[i].read(cx).is_focused(window);
        let frame = div()
            .flex_1()
            .h(sized(28.))
            .px_2()
            .flex()
            .items_center()
            .gap_1()
            .rounded_md()
            .border_1()
            .border_color(if focused { theme.accent } else { theme.border })
            .bg(theme.bg)
            .child(div().flex_1().min_w_0().child(b.inputs[i].clone()));
        // A field that says what it takes (ADR-137): its `?`, F1, and the
        // list under it.
        let frame = match b.helps.get(i) {
            Some(help) => crate::reference::with_help(
                frame,
                help,
                &b.inputs[i],
                shell.keys_for("ui.field_reference", &serde_json::Value::Null),
                window,
                cx,
            ),
            None => frame,
        };
        form = form.child(
            div()
                .flex()
                .items_center()
                .gap_2()
                .child(
                    div()
                        .flex_none()
                        .w(column)
                        .text_body()
                        .text_color(theme.text_muted)
                        .child(label),
                )
                .child(frame),
        );
    }
    form = form.child(
        div()
            .flex()
            .items_center()
            .gap_2()
            .child(
                div()
                    .flex_none()
                    .w(column)
                    .text_body()
                    .text_color(theme.text_muted)
                    .child(case),
            )
            .child(case_control(b, &theme, cx)),
    );
    // The renames scroll under a scrollbar; how many more there are is
    // said below them, never cut off with the rows the box cannot hold.
    let mut list = div()
        .id("rename-preview")
        .flex()
        .flex_col()
        .max_h(sized(260.))
        .track_scroll(&b.scroll)
        .overflow_y_scroll()
        .text_body()
        .font_family(theme.code_font.clone());
    // What the regex matched, and what it put there, on a value's tint.
    let mark = gpui::HighlightStyle {
        background_color: Some(theme.term),
        ..gpui::HighlightStyle::default()
    };
    for (i, renamed) in b.preview.iter().enumerate() {
        let Renamed {
            from,
            to,
            matched,
            put,
        } = renamed;
        // Each name drawn alone, its controls shown, the marks on what is
        // drawn.
        let (from_drawn, matched) = crate::views::entry::DrawnName::marked(from, matched);
        let (to_drawn, put) = crate::views::entry::DrawnName::marked(to, put);
        list = list.child(
            div()
                .flex()
                .gap_2()
                .child(clipped_marked(
                    div()
                        .id(crate::elements::known_by("rename-from", (i, from)))
                        .flex_1()
                        .min_w_0()
                        .truncate()
                        .text_color(theme.text_muted),
                    from_drawn,
                    &matched,
                    mark,
                ))
                .child(div().flex_none().child(crate::sides::becomes()))
                .child(clipped_marked(
                    // Known by what it says: the pattern typed on, a name
                    // pointed at does not keep the one before's tooltip.
                    div()
                        .id(crate::elements::known_by("rename-to", (i, to)))
                        .flex_1()
                        .min_w_0()
                        .truncate(),
                    to_drawn,
                    &put,
                    mark,
                )),
        );
    }
    let list =
        div()
            .relative()
            .flex()
            .flex_col()
            .child(list)
            .child(crate::elements::scrollbar::vertical(
                "rename-preview-scrollbar",
                crate::elements::scrollbar::Scrolls::Offset(b.scroll.clone()),
            ));
    // Nothing to rename is said where the renames would be.
    let unchanged = b.changes_nothing().then(|| {
        div().text_body().text_color(theme.text_muted).child(
            words!(
                renamebulk_no_changes = "No name changes",
                note = "Where the bulk rename lists its renames, when it would rename \
                            nothing."
            )
            .say(),
        )
    });
    let more = (b.renamed > b.preview.len()).then(|| {
        div().text_small().text_color(theme.text_muted).child(
            words!(
                renamebulk_more = "{ $count ->
                    [one] … { $count } more
                   *[other] … { $count } more
                }",
                count = b.renamed - b.preview.len(),
                note = "Under the renames the bulk rename lists: how many more it makes."
            )
            .say(),
        )
    });
    let panel = modal::panel("bulk-rename", 720., &theme)
        .aria_label(
            words!(
                renamebulk_dialog = "Bulk rename",
                note = "What a screen reader calls the dialog renaming several files at once."
            )
            .say(),
        )
        .p_3()
        .gap_2()
        .key_context(keymap::BULK)
        .child(
            div().text_title().child(
                words!(
                    renamebulk_title = "{ $count ->
                            [one] Rename 1 item
                           *[other] Rename { $count } items
                        }",
                    count = b.count,
                    note = "The title of the dialog renaming several files at once."
                )
                .say(),
            ),
        )
        .child(form)
        .children(
            b.error
                .as_ref()
                .map(|e| div().text_body().text_color(theme.error).child(e.say())),
        )
        .child(list)
        .children(unchanged)
        .children(more)
        .child(modal::footer(
            footer_hint(shell),
            [
                modal::pressed(
                    controls::button(
                        "bulk-cancel",
                        words!(
                            renamebulk_cancel = "Cancel",
                            note = "A button: close the bulk rename, renaming nothing."
                        )
                        .say(),
                        None,
                        ButtonStyle::Plain,
                        &b.cancel_focus,
                        &theme,
                    ),
                    Shell::close_bulk,
                    cx,
                ),
                modal::pressed(
                    // While nothing would be renamed (no name changes, an
                    // error, a preview not back yet): said, not offered,
                    // as a question's answer is.
                    if b.applies() {
                        controls::button(
                            "bulk-apply",
                            rename().say(),
                            Some("check"),
                            ButtonStyle::Plain,
                            &b.apply_focus,
                            &theme,
                        )
                    } else {
                        controls::inert_button_with(
                            "bulk-apply",
                            rename().say(),
                            Some("check"),
                            &b.apply_focus,
                            &theme,
                        )
                    },
                    Shell::bulk_apply,
                    cx,
                ),
            ],
            &theme,
        ));
    Some(modal::modal(panel, 60., 4, Shell::close_bulk, &theme, cx))
}

/// The label of the case's control.
fn case_title() -> Words {
    words!(
        renamebulk_case = "Case",
        note = "The bulk rename's choice of the case it gives names."
    )
}

/// The bulk rename's button that renames.
fn rename() -> Words {
    words!(
        renamebulk_rename = "Rename",
        note = "A button: rename the files as the bulk rename lists."
    )
}

/// The dialog's keys, as the keymap binds them: those it has no key for
/// are left out.
fn footer_hint(shell: &Shell) -> String {
    let none = serde_json::Value::Null;
    let case = shell.keys_for("ui.rename_bulk_case", &none);
    let reference = shell.keys_for("ui.field_reference", &none);
    match (case, reference) {
        (Some(case), Some(reference)) => words!(
            renamebulk_keys_case_reference = "Tab switches fields · { $case } changes case · \
                                              { $reference } says what one takes · Enter \
                                              renames (undoable) · Escape cancels",
            case = skwir_core::words::Keys(case),
            reference = skwir_core::words::Keys(reference),
            note = "The bulk rename's keys. case: the keys that change the case (Ctrl+K); \
                    reference: the keys that say what a field takes (F1)."
        ),
        (Some(case), None) => words!(
            renamebulk_keys_case = "Tab switches fields · { $case } changes case · Enter \
                                    renames (undoable) · Escape cancels",
            case = skwir_core::words::Keys(case),
            note = "The bulk rename's keys. case: the keys that change the case (Ctrl+K)."
        ),
        (None, Some(reference)) => words!(
            renamebulk_keys_reference = "Tab switches fields · { $reference } says what one \
                                         takes · Enter renames (undoable) · Escape cancels",
            reference = skwir_core::words::Keys(reference),
            note = "The bulk rename's keys. reference: the keys that say what a field takes \
                    (F1)."
        ),
        (None, None) => words!(
            renamebulk_keys = "Tab switches fields · Enter renames (undoable) · Escape cancels",
            note = "The bulk rename's keys."
        ),
    }
    .say()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The regex and the replacement are taken as typed: a recipe's
    /// replacement that is a space turns underscores into spaces, never
    /// takes them away; the template and the start without what is around
    /// them; an empty field asks nothing.
    #[test]
    fn the_fields_ask_what_is_typed() {
        let texts = |t: [&str; 4]| t.map(str::to_owned);
        let pane = PaneId(1);
        let spec = spec_of(&texts([" {full} ", "_", " ", " 7 "]), 2, pane);
        assert_eq!(spec["template"], "{full}");
        assert_eq!(spec["regex"], "_");
        assert_eq!(spec["replacement"], " ");
        assert_eq!(spec["start"], 7);
        assert_eq!(spec["case"], "upper");
        let spec = spec_of(&texts(["", "", "", "x"]), 0, pane);
        assert!(spec["template"].is_null() && spec["regex"].is_null());
        assert!(spec["replacement"].is_null() && spec["case"].is_null());
        assert_eq!(spec["start"], 1);
    }

    /// The regex's and the replacement's references, which read the first
    /// name alone, are asked once for it while the dialog is open, and
    /// again for another; until one was kept, at each change.
    #[test]
    fn what_reads_the_first_name_alone_is_asked_once_for_it() {
        assert!(asks_again(Some("a.txt"), None), "none kept yet");
        assert!(!asks_again(Some("a.txt"), Some("a.txt")), "kept for it");
        assert!(asks_again(Some("b.txt"), Some("a.txt")), "another name");
        assert!(!asks_again(None, Some("a.txt")), "no preview: kept");
        assert!(asks_again(None, None));
    }

    /// The arrows step the case and stop at its ends; Ctrl+K and Space
    /// give the next, round the last. Each but Unchanged asks the plugin
    /// for its own value.
    #[test]
    fn the_case_steps_and_goes_round() {
        let last = CASES.len() - 1;
        assert_eq!(case_stepped(0, Some(-1)), 0, "stops at the first");
        assert_eq!(case_stepped(0, Some(1)), 1);
        assert_eq!(case_stepped(last, Some(1)), last, "stops at the last");
        assert_eq!(case_stepped(2, Some(-1)), 1);
        assert_eq!(case_stepped(0, None), 1);
        assert_eq!(case_stepped(last, None), 0, "round the last");
        let texts = ["{full}", "", "", ""].map(str::to_owned);
        for (at, &value) in CASES.iter().enumerate().skip(1) {
            assert_eq!(spec_of(&texts, at, PaneId(1))["case"], value);
        }
    }
}
