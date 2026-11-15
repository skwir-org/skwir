//! Bulk rename dialog: template, regex, replacement, case, start; live
//! preview from `rename_bulk.preview`, where the regex matched in each old
//! name and what it put in the new one tinted; Enter or Rename applies
//! `rename_bulk`. The template, the regex and the replacement each say
//! what they take (ADR-137): the plugin's `rename_bulk.reference`, asked
//! with each preview, opened from the `?` inside the field at its end or
//! F1 in it, and what goes on with a part begun offered under the field
//! as it is typed (`reference.rs`). It opens on `{full}`, which leaves
//! every name as it is. The case is a row of its values side by side, a
//! dropdown where they do not fit (`case_control`).

use std::ops::Range;
use std::rc::Rc;

use crate::sizes::TextSizes;
use crate::sizes::sized;
use gpui::{AnyElement, Context, Entity, Subscription, Window, div, prelude::*};
use skwir_core::EntryId;
use skwir_core::command::{Invocation, Outcome};
use skwir_core::reference::Reference;

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

/// The cases a rename gives names: the value the plugin knows, and the
/// label, written as the case leaves a name. The first asks for none.
pub const CASES: [(&str, &str); 5] = [
    ("none", "Unchanged"),
    ("lower", "lowercase"),
    ("upper", "UPPERCASE"),
    ("title", "Title Case"),
    ("sentence", "Sentence case"),
];
/// Each field: the key the plugin knows it by, its label, and what it
/// shows while empty.
const FIELDS: [(&str, &str, &str); 4] = [
    ("template", "Template", "{name}-{n:02}{.ext}"),
    ("regex", "Regex", "Optional: a pattern over the whole name"),
    (
        "replacement",
        "Replacement",
        "Optional: what each match becomes, $1 for its first group",
    ),
    ("start", "Start", "1"),
];

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
    renamed: usize,
    pub error: Option<String>,
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
        let at = FIELDS.iter().position(|(named, ..)| *named == key)?;
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
            self.set_status(crate::shell::NOTHING_SELECTED, false, cx);
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
        for (i, &(_, label, placeholder)) in FIELDS.iter().enumerate() {
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
            helps: std::array::from_fn(|i| FieldHelp::new(FIELDS[i].0, None)),
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
        self.off_thread_until(
            self.pane_lane(pane),
            "rename-bulk-preview",
            cancel,
            move || {
                // What each field takes, given the first item as the rename
                // stands; one the plugin could not say keeps the last.
                let references: [Option<Reference>; SAID] = std::array::from_fn(|i| {
                    let mut asked = said.clone();
                    asked["field"] = FIELDS[i].0.into();
                    match app.dispatch(&Invocation::with_args("rename_bulk.reference", &asked)) {
                        Ok(Outcome::Value(v)) => serde_json::from_value(v).ok(),
                        Ok(_) => None,
                        Err(e) => {
                            tracing::warn!("rename_bulk.reference ({}): {e:#}", FIELDS[i].0);
                            None
                        }
                    }
                });
                let (shown, count, error) =
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
                            (shown, count, None)
                        }
                        Ok(_) => (Vec::new(), 0, None),
                        Err(e) => (Vec::new(), 0, Some(format!("{e:#}"))),
                    };
                (shown, count, error, references)
            },
            move |this, result, _, cx| {
                let (preview, renamed, error, references) = result;
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
        b.error = Some("The selection changed since this opened: open it again".into());
        b.current = true;
        cx.notify();
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
        Some(&(value, _)) if case > 0 => serde_json::Value::String(value.into()),
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
        .aria_label("Case")
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
    for (at, &(_, label)) in CASES.iter().enumerate() {
        let focus = b.case_focus.clone();
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
    let control = controls::dropdown("bulk-case", CASES[b.case].1, &b.case_focus, theme)
        .aria_label("Case")
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
    for (at, &(_, label)) in CASES.iter().enumerate() {
        let focus = b.case_focus.clone();
        list = list.child(
            controls::dropdown_row(format!("bulk-case-row-{at}"), at == b.case, theme)
                .child(label)
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

pub fn render(shell: &Shell, window: &Window, cx: &mut Context<Shell>) -> Option<AnyElement> {
    let b = shell.bulk.as_ref()?;
    let theme = cx.global::<Theme>().clone();
    let mut form = div().flex().flex_col().gap_2();
    for (i, (_, label, _)) in FIELDS.iter().enumerate() {
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
                        .w(sized(100.))
                        .text_body()
                        .text_color(theme.text_muted)
                        .child(*label),
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
                    .w(sized(100.))
                    .text_body()
                    .text_color(theme.text_muted)
                    .child("Case"),
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
                    from.clone(),
                    matched,
                    mark,
                ))
                .child(div().flex_none().child("→"))
                .child(clipped_marked(
                    // Known by what it says: the pattern typed on, a name
                    // pointed at does not keep the one before's tooltip.
                    div()
                        .id(crate::elements::known_by("rename-to", (i, to)))
                        .flex_1()
                        .min_w_0()
                        .truncate(),
                    to.clone(),
                    put,
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
        div()
            .text_body()
            .text_color(theme.text_muted)
            .child("No name changes")
    });
    let more = (b.renamed > b.preview.len()).then(|| {
        div()
            .text_small()
            .text_color(theme.text_muted)
            .child(format!("… {} more", b.renamed - b.preview.len()))
    });
    let panel = modal::panel("bulk-rename", 720., &theme)
        .aria_label("Bulk rename")
        .p_3()
        .gap_2()
        .key_context(keymap::BULK)
        .child(
            div()
                .text_title()
                .child(format!("Rename {}", crate::format::count(b.count, "item"))),
        )
        .child(form)
        .children(
            b.error
                .clone()
                .map(|e| div().text_body().text_color(theme.error).child(e)),
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
                        "Cancel",
                        None,
                        ButtonStyle::Plain,
                        &b.cancel_focus,
                        &theme,
                    ),
                    Shell::close_bulk,
                    cx,
                ),
                modal::pressed(
                    controls::button(
                        "bulk-apply",
                        "Rename",
                        Some("check"),
                        ButtonStyle::Plain,
                        &b.apply_focus,
                        &theme,
                    )
                    .when(!b.applies(), |d| d.opacity(0.5)),
                    Shell::bulk_apply,
                    cx,
                ),
            ],
            &theme,
        ));
    Some(modal::modal(panel, 60., 4, Shell::close_bulk, &theme, cx))
}

/// The dialog's keys, as the keymap binds them: those it has no key for
/// are left out.
fn footer_hint(shell: &Shell) -> String {
    let none = serde_json::Value::Null;
    let mut said = vec!["Tab switches fields".to_owned()];
    if let Some(keys) = shell.keys_for("ui.rename_bulk_case", &none) {
        said.push(format!("{keys} changes case"));
    }
    if let Some(keys) = shell.keys_for("ui.field_reference", &none) {
        said.push(format!("{keys} says what one takes"));
    }
    said.push("Enter renames (undoable)".to_owned());
    said.push("Escape cancels".to_owned());
    said.join(" · ")
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
        for (at, &(value, _)) in CASES.iter().enumerate().skip(1) {
            assert_eq!(spec_of(&texts, at, PaneId(1))["case"], value);
        }
    }
}
