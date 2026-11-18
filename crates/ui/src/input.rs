//! A single-line text input entity: cursor, selection, IME-safe editing,
//! clipboard, word moves. Reused by the address bar, the palette, and prompts.
//! Modeled on gpui's `examples/input.rs`.
//!
//! Its words are a person's in any script (`skwir_core::segment`, ICU4X's):
//! Ctrl and the arrows, Ctrl+Backspace and Ctrl+Delete go by them, a double
//! click selects one and a drag after it goes on a word at a time, a triple
//! click selects the whole text, and a screen reader is told where each
//! begins. Chinese, Japanese and Thai, written without spaces, are words
//! as English is. A secret is one word: nothing tells where its words are.

use std::ops::Range;

use gpui::{
    App, Bounds, ClipboardItem, Context, CursorStyle, Element, ElementId, ElementInputHandler,
    Entity, EntityInputHandler, EventEmitter, FocusHandle, Focusable, GlobalElementId,
    InspectorElementId, IntoElement, KeyBinding, LayoutId, MouseButton, MouseDownEvent,
    MouseMoveEvent, MouseUpEvent, PaintQuad, Pixels, Point, ShapedLine, SharedString, Style,
    TextRun, UTF16Selection, UnderlineStyle, Window, actions, div, fill, point, prelude::*, px,
    relative, size,
};
use unicode_segmentation::UnicodeSegmentation;

use crate::theme::Theme;
use skwir_core::segment;
use skwir_core::words::Words;

actions!(
    text_input,
    [
        Backspace,
        Delete,
        Left,
        Right,
        SelectLeft,
        SelectRight,
        WordLeft,
        WordRight,
        SelectWordLeft,
        SelectWordRight,
        SelectAll,
        Home,
        End,
        SelectHome,
        SelectEnd,
        Paste,
        Cut,
        Copy,
        Submit,
        Cancel,
        Undo,
        Redo,
        DeleteWordLeft,
        DeleteWordRight,
    ]
);

/// Key context every text input renders with.
pub const KEY_CONTEXT: &str = "TextInput";

/// A binding of Skwir's own keys whose `secondary-` is this system's
/// (`KeyBinding::new` takes none).
fn for_os<A: gpui::Action>(keys: &'static str, action: A, context: Option<&str>) -> KeyBinding {
    crate::keymap::binding(keys, action, context).expect("Skwir's own keys parse")
}

/// Bindings for the input's own actions, scoped to [`KEY_CONTEXT`].
pub fn key_bindings() -> Vec<KeyBinding> {
    let c = Some(KEY_CONTEXT);
    vec![
        KeyBinding::new("backspace", Backspace, c),
        KeyBinding::new("delete", Delete, c),
        KeyBinding::new("left", Left, c),
        KeyBinding::new("right", Right, c),
        KeyBinding::new("shift-left", SelectLeft, c),
        KeyBinding::new("shift-right", SelectRight, c),
        KeyBinding::new("ctrl-left", WordLeft, c),
        KeyBinding::new("ctrl-right", WordRight, c),
        KeyBinding::new("ctrl-shift-left", SelectWordLeft, c),
        KeyBinding::new("ctrl-shift-right", SelectWordRight, c),
        for_os("secondary-a", SelectAll, c),
        KeyBinding::new("home", Home, c),
        KeyBinding::new("end", End, c),
        KeyBinding::new("shift-home", SelectHome, c),
        KeyBinding::new("shift-end", SelectEnd, c),
        for_os("secondary-v", Paste, c),
        for_os("secondary-c", Copy, c),
        for_os("secondary-x", Cut, c),
        KeyBinding::new("ctrl-backspace", DeleteWordLeft, c),
        KeyBinding::new("ctrl-delete", DeleteWordRight, c),
        // The field's own undo, before the window's (the file operations').
        for_os("secondary-z", Undo, c),
        for_os("secondary-shift-z", Redo, c),
        KeyBinding::new("ctrl-y", Redo, c),
        KeyBinding::new("enter", Submit, c),
        KeyBinding::new("escape", Cancel, c),
    ]
}

const BULLET: char = '•';

/// The UTF-8 byte offset in `text` of UTF-16 offset `offset`.
fn utf16_to_utf8(text: &str, offset: usize) -> usize {
    let mut utf16 = 0;
    for (index, ch) in text.char_indices() {
        if utf16 >= offset {
            return index;
        }
        utf16 += ch.len_utf16();
    }
    text.len()
}

/// What an input tells its owner.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum InputEvent {
    /// Enter was pressed.
    Submit,
    /// Escape was pressed.
    Cancel,
    /// The text changed.
    Changed,
}

/// Right or End at the end of the text while a hint follows it: the hint
/// is taken (`TextInput::set_hint`). Only a field given hints sends it.
pub struct HintTaken;

/// Backspace or Left with the caret at the start of the text, nothing
/// selected: what stands before the text (the address field's scope) is
/// acted on. Sent only by a field that asked (`TextInput::send_start`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AtStart {
    Backspace,
    Left,
}

/// How a part of the text is drawn (`TextInput::set_styles`): its color, a
/// tint behind it, a wavy line under it.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Paint {
    pub color: Option<gpui::Hsla>,
    pub background: Option<gpui::Hsla>,
    pub wavy: Option<gpui::Hsla>,
}

/// The text and selection an edit started from.
type Snapshot = (SharedString, Range<usize>);

/// One edit made in several fields at once (`TextInput::replace_all_together`:
/// a recipe's pattern and its replacement), which Ctrl+Z in any of them
/// takes back in all, and Redo puts back in all: known by its id in each
/// field's history, with the other fields it was made in.
#[derive(Clone)]
struct Together {
    id: u64,
    with: Vec<gpui::WeakEntity<TextInput>>,
}

/// A step of a field's history: the text and selection it goes back to,
/// and the edit of several fields it is part of, if it is.
type Step = (Snapshot, Option<Together>);

/// What Ctrl+Z and Ctrl+Shift+Z walk through. A run of characters typed
/// one after the other is one step, as in any text field; anything else
/// (a deletion, a paste, a composition, typing somewhere else) starts one.
#[derive(Default)]
struct History {
    undo: Vec<Step>,
    redo: Vec<Step>,
    /// Where the last typed character ended, while typing continues there.
    typed_end: Option<usize>,
}

impl History {
    /// Most fields hold a line; this is more steps than anyone takes back.
    const DEPTH: usize = 100;

    /// Before an edit of `before`. `typed` is where a character typed at
    /// the cursor goes (`typed_at`); a run of those joins one step.
    fn record(&mut self, before: Snapshot, typed: Option<Range<usize>>) {
        let continues = typed
            .as_ref()
            .is_some_and(|t| self.typed_end == Some(t.start));
        if !continues {
            self.push(before, None);
        }
        self.redo.clear();
        self.typed_end = typed.map(|t| t.end);
    }

    /// Before an edit of `before` made in other fields too (`together`):
    /// a step of its own, never joined to what is typed after it.
    fn record_together(&mut self, before: Snapshot, together: Together) {
        self.push(before, Some(together));
        self.redo.clear();
        self.typed_end = None;
    }

    fn push(&mut self, before: Snapshot, together: Option<Together>) {
        if self.undo.len() == Self::DEPTH {
            self.undo.remove(0);
        }
        self.undo.push((before, together));
    }

    /// The last step taken back, `current` kept for Redo, and the edit of
    /// several fields it was part of.
    fn undo(&mut self, current: Snapshot) -> Option<Step> {
        let (previous, together) = self.undo.pop()?;
        self.redo.push((current, together.clone()));
        self.typed_end = None;
        Some((previous, together))
    }

    fn redo(&mut self, current: Snapshot) -> Option<Step> {
        let (next, together) = self.redo.pop()?;
        self.undo.push((current, together.clone()));
        self.typed_end = None;
        Some((next, together))
    }

    /// [`History::undo`] where the last step is the edit `id` made in
    /// several fields, taken back in another of them; nothing where this
    /// field was edited since (its own edit is never taken back for it).
    fn undo_together(&mut self, id: u64, current: Snapshot) -> Option<Snapshot> {
        self.undo
            .last()
            .is_some_and(|(_, together)| together.as_ref().is_some_and(|t| t.id == id))
            .then(|| self.undo(current).map(|(previous, _)| previous))?
    }

    /// [`History::redo`] alike, for the edit `id` put back elsewhere.
    fn redo_together(&mut self, id: u64, current: Snapshot) -> Option<Snapshot> {
        self.redo
            .last()
            .is_some_and(|(_, together)| together.as_ref().is_some_and(|t| t.id == id))
            .then(|| self.redo(current).map(|(next, _)| next))?
    }

    fn clear(&mut self) {
        *self = History::default();
    }
}

/// The most characters a field holds of what is typed, pasted or composed
/// in it, as Qt's line edit holds by default, of a completion taken
/// (`TextInput::replace_all`), and of a text a command gives it
/// ([`held`]). Its line is shaped whole on the window's thread at each
/// change: 20,000 copied files pasted in the address (600 KB of paths)
/// held the window half a minute.
pub const MOST_CHARS: usize = 32_767;

/// What of `text`, a field's whole text given by a command (`ui.search`'s
/// `text`), the field takes, as it would were it pasted in it empty: whole
/// graphemes, [`MOST_CHARS`] at most.
pub fn held(mut text: String) -> String {
    let end = fitted("", &(0..0), &text).len();
    text.truncate(end);
    text
}

/// What of `text`, put in place of `range` in `content`, the field takes:
/// whole graphemes while the field stays within [`MOST_CHARS`].
fn fitted<'a>(content: &str, range: &Range<usize>, text: &'a str) -> &'a str {
    let kept = content[..range.start].chars().count() + content[range.end..].chars().count();
    let room = MOST_CHARS.saturating_sub(kept);
    if text.len() <= room {
        // As many bytes as there is room for characters.
        return text;
    }
    let mut chars = 0;
    let mut end = 0;
    for (at, grapheme) in text.grapheme_indices(true) {
        chars += grapheme.chars().count();
        if chars > room {
            break;
        }
        end = at + grapheme.len();
    }
    &text[..end]
}

/// Whether putting `text` in place of `range` leaves a field as it is (a
/// key typed or a paste in a field full to [`MOST_CHARS`], Delete at the
/// end): no step to take back, nothing to tell, no line to lay out again.
/// Not while `composing`, which the edit ends.
fn changes_nothing(range: &Range<usize>, text: &str, composing: bool) -> bool {
    text.is_empty() && range.is_empty() && !composing
}

/// Where `text` replacing `range` lands, if it is one character typed at
/// the cursor; a space ends a run, so words come back one at a time.
fn typed_at(range: &Range<usize>, text: &str) -> Option<Range<usize>> {
    let mut chars = text.chars();
    let typed = range.is_empty()
        && chars.next().is_some_and(|c| !c.is_whitespace())
        && chars.next().is_none();
    typed.then(|| range.start..range.start + text.len())
}

#[allow(clippy::struct_excessive_bools)] // Independent states of a text field: masked, selecting, the selection's direction, what it says at its start.
pub struct TextInput {
    focus_handle: FocusHandle,
    content: SharedString,
    /// What an empty field shows, said where it is drawn.
    placeholder: Words,
    /// What assistive technology calls it, when the placeholder does not
    /// name it (`set_label`), said where it is drawn.
    label: Option<Words>,
    /// Show a bullet per character, and keep the text off the clipboard.
    masked: bool,
    selected_range: Range<usize>,
    selection_reversed: bool,
    marked_range: Option<Range<usize>>,
    last_layout: Option<ShapedLine>,
    last_bounds: Option<Bounds<Pixels>>,
    /// How far the line is scrolled left, so a text longer than the field
    /// keeps the cursor in view (`scrolled`, in prepaint).
    scroll_x: Pixels,
    is_selecting: bool,
    /// The word a double click selected, while the drag it starts goes on
    /// a word at a time (`drag_select`).
    selecting_words: Option<Range<usize>>,
    /// Where the pointer is while a drag selects, followed wherever it goes
    /// in the window (`TextElement::paint`).
    dragged_at: Option<Point<Pixels>>,
    /// Held past either end of the field, the selection goes on there and
    /// the line scrolls, a step at a time (`autoscroll_step`), until the
    /// pointer comes back, the button is let go or the text ends.
    autoscroll: Option<gpui::Task<()>>,
    /// Kept only for text that is not masked: a passphrase leaves no trail.
    history: History,
    /// What the text may go on with, drawn muted after it while the cursor
    /// is at its end (`set_hint`).
    hint: Option<SharedString>,
    /// Parts of the text drawn otherwise, by byte range (`set_styles`).
    styles: Vec<(Range<usize>, Paint)>,
    /// Whether Backspace and Left at the start are said (`AtStart`).
    start_events: bool,
}

impl EventEmitter<InputEvent> for TextInput {}

impl EventEmitter<HintTaken> for TextInput {}

impl EventEmitter<AtStart> for TextInput {}

impl TextInput {
    pub fn new(placeholder: impl Into<Words>, cx: &mut Context<Self>) -> Self {
        TextInput {
            focus_handle: cx.focus_handle(),
            content: "".into(),
            placeholder: placeholder.into(),
            label: None,
            masked: false,
            selected_range: 0..0,
            selection_reversed: false,
            marked_range: None,
            last_layout: None,
            last_bounds: None,
            scroll_x: px(0.),
            is_selecting: false,
            selecting_words: None,
            dragged_at: None,
            autoscroll: None,
            history: History::default(),
            hint: None,
            styles: Vec::new(),
            start_events: false,
        }
    }

    /// Draw these parts of the text otherwise, by byte range into it, until
    /// other styles are set or the text is replaced; ranges that do not fit
    /// the text are left out. An edit keeps those wholly before it, which
    /// still cover the same characters, until its owner reads the text
    /// again: a key never takes them all away for a frame.
    pub fn set_styles(&mut self, styles: Vec<(Range<usize>, Paint)>, cx: &mut Context<Self>) {
        if styles != self.styles {
            self.styles = styles;
            cx.notify();
        }
    }

    /// Say Backspace and Left at the start of the text (`AtStart`), where
    /// something stands before it.
    pub fn send_start(&mut self, on: bool) {
        self.start_events = on;
    }

    /// Where the caret is, a byte offset into the text.
    pub fn caret(&self) -> usize {
        self.cursor_offset()
    }

    /// The caret is at the start, nothing selected.
    fn at_start(&self) -> bool {
        self.start_events && self.selected_range == (0..0)
    }

    /// Its name for assistive technology, where its placeholder is a hint
    /// rather than a name ("Address" for "Path, query:… or location").
    pub fn set_label(&mut self, label: impl Into<Words>) {
        self.label = Some(label.into());
    }

    /// For secrets: bullets on screen, no copy or cut.
    pub fn set_masked(&mut self, masked: bool) {
        self.masked = masked;
    }

    /// Byte offset in the text for a byte offset in what is displayed.
    fn text_offset(&self, offset: usize) -> usize {
        if !self.masked {
            // The layout is the last paint's: the text may have changed
            // since.
            return self.snap(offset);
        }
        self.content
            .char_indices()
            .nth(offset / BULLET.len_utf8())
            .map_or(self.content.len(), |(index, _)| index)
    }

    /// Byte offset in what is displayed for a byte offset in the text.
    fn shown_offset(&self, offset: usize) -> usize {
        if self.masked {
            self.content[..offset.min(self.content.len())]
                .chars()
                .count()
                * BULLET.len_utf8()
        } else {
            offset
        }
    }

    /// What an empty field shows.
    pub fn set_placeholder(&mut self, placeholder: impl Into<Words>, cx: &mut Context<Self>) {
        let placeholder = placeholder.into();
        if placeholder != self.placeholder {
            self.placeholder = placeholder;
            cx.notify();
        }
    }

    pub fn text(&self) -> &str {
        &self.content
    }

    /// Replace the whole text and put the cursor at the end.
    pub fn set_text(&mut self, text: impl Into<SharedString>, cx: &mut Context<Self>) {
        self.content = text.into();
        self.styles.clear();
        // Text put here is where editing starts, not a step to take back.
        self.history.clear();
        let end = self.content.len();
        self.selected_range = end..end;
        self.selection_reversed = false;
        self.marked_range = None;
        cx.notify();
    }

    /// Replace the whole text as an edit, the cursor at its end: Ctrl+Z
    /// takes it back (a completion taken). Held to [`MOST_CHARS`], as
    /// what is typed is.
    pub fn replace_all(&mut self, text: &str, cx: &mut Context<Self>) {
        self.replace_all_as(text, None, cx);
    }

    /// Write each text whole in its field, as one edit of them all: Ctrl+Z
    /// in any of them takes it back in each where it is still the last
    /// edit, and Redo puts it back alike (a recipe's pattern and its
    /// replacement, ADR-137). A form that fills several fields at once
    /// fills them so.
    pub fn replace_all_together(fields: &[(Entity<TextInput>, &str)], cx: &mut App) {
        static EDITS: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);
        if let [(field, text)] = fields {
            field.update(cx, |field, cx| field.replace_all(text, cx));
            return;
        }
        let id = EDITS.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        for (at, (field, text)) in fields.iter().enumerate() {
            let with = fields
                .iter()
                .enumerate()
                .filter(|&(other, _)| other != at)
                .map(|(_, (other, _))| other.downgrade())
                .collect();
            field.update(cx, |field, cx| {
                field.replace_all_as(text, Some(Together { id, with }), cx);
            });
        }
    }

    /// [`TextInput::replace_all`], as part of the edit `together` when it
    /// is one of several fields'.
    fn replace_all_as(&mut self, text: &str, together: Option<Together>, cx: &mut Context<Self>) {
        let text = fitted("", &(0..0), text);
        if !self.masked {
            let before = (self.content.clone(), self.selected_range.clone());
            match together {
                Some(together) => self.history.record_together(before, together),
                None => self.history.record(before, None),
            }
        }
        self.content = text.to_owned().into();
        self.styles.clear();
        let end = self.content.len();
        self.selected_range = end..end;
        self.selection_reversed = false;
        self.marked_range = None;
        cx.emit(InputEvent::Changed);
        cx.notify();
    }

    /// Put `text` where the caret is, over what is selected, as an edit
    /// Ctrl+Z takes back, the caret after it: what a field's reference
    /// writes (ADR-137). Held to [`MOST_CHARS`], as what is typed is.
    pub fn put(&mut self, text: &str, cx: &mut Context<Self>) {
        let range = self.snap(self.selected_range.start)..self.snap(self.selected_range.end);
        let text = fitted(&self.content, &range, text);
        if changes_nothing(&range, text, false) {
            return;
        }
        if !self.masked {
            let before = (self.content.clone(), self.selected_range.clone());
            self.history.record(before, None);
        }
        self.content =
            (self.content[..range.start].to_owned() + text + &self.content[range.end..]).into();
        kept_before(&mut self.styles, range.start);
        let end = range.start + text.len();
        self.selected_range = end..end;
        self.selection_reversed = false;
        self.marked_range = None;
        cx.emit(InputEvent::Changed);
        cx.notify();
    }

    /// What the text may go on with (a completion): drawn muted after it
    /// while the cursor is at its end with nothing selected, taken by
    /// Right or End there (`HintTaken`). The hint is not the text: what
    /// the field holds stays what was typed.
    pub fn set_hint(&mut self, hint: Option<String>, cx: &mut Context<Self>) {
        let hint = hint.filter(|h| !h.is_empty()).map(SharedString::from);
        if hint != self.hint {
            self.hint = hint;
            cx.notify();
        }
    }

    /// The hint, when it shows: the cursor at the end, nothing selected or
    /// being composed.
    fn hint_shown(&self) -> Option<&SharedString> {
        let end = self.content.len();
        (self.selected_range == (end..end) && self.marked_range.is_none() && !self.masked)
            .then_some(self.hint.as_ref())
            .flatten()
    }

    pub fn select_all_now(&mut self, cx: &mut Context<Self>) {
        self.selected_range = 0..self.content.len();
        self.selection_reversed = false;
        cx.notify();
    }

    /// `offset` as an offset in the text: within it, and at the start of
    /// the character it falls in. Every offset that does not come from the
    /// input's own edits goes through it: an edit at one that splits a
    /// character panics.
    fn snap(&self, offset: usize) -> usize {
        snapped(&self.content, offset)
    }

    /// Select `range` (byte offsets), e.g. the stem of a file name. Given
    /// from outside (`ui.ask`'s `select`, an agent's), each end is snapped
    /// to a character.
    pub fn select_range(&mut self, range: Range<usize>, cx: &mut Context<Self>) {
        let end = self.snap(range.end);
        let start = self.snap(range.start.min(end));
        self.selected_range = start..end;
        self.selection_reversed = false;
        cx.notify();
    }

    pub fn focus(&self, window: &mut Window, cx: &mut gpui::App) {
        window.focus(&self.focus_handle, cx);
    }

    pub fn is_focused(&self, window: &Window) -> bool {
        self.focus_handle.is_focused(window)
    }

    // ------------------------------------------------------------ actions

    fn left(&mut self, _: &Left, _: &mut Window, cx: &mut Context<Self>) {
        // What stands before the text is on its left where the caret's
        // left is the field's: not at the start of a text read right to
        // left, which is its right end.
        if self.at_start() && self.drawn_beside(0, true).is_none() {
            cx.emit(AtStart::Left);
        } else if self.selected_range.is_empty() {
            self.move_to(self.arrow_from(self.cursor_offset(), true), cx);
        } else {
            self.move_to(self.selection_end_on(true), cx);
        }
    }

    fn right(&mut self, _: &Right, _: &mut Window, cx: &mut Context<Self>) {
        if self.hint_shown().is_some() {
            cx.emit(HintTaken);
        } else if self.selected_range.is_empty() {
            self.move_to(self.arrow_from(self.selected_range.end, false), cx);
        } else {
            self.move_to(self.selection_end_on(false), cx);
        }
    }

    fn select_left(&mut self, _: &SelectLeft, _: &mut Window, cx: &mut Context<Self>) {
        self.select_to(self.arrow_from(self.cursor_offset(), true), cx);
    }

    fn select_right(&mut self, _: &SelectRight, _: &mut Window, cx: &mut Context<Self>) {
        self.select_to(self.arrow_from(self.cursor_offset(), false), cx);
    }

    fn word_left(&mut self, _: &WordLeft, _: &mut Window, cx: &mut Context<Self>) {
        self.move_to(self.word_from(self.cursor_offset(), true), cx);
    }

    fn word_right(&mut self, _: &WordRight, _: &mut Window, cx: &mut Context<Self>) {
        self.move_to(self.word_from(self.cursor_offset(), false), cx);
    }

    fn select_word_left(&mut self, _: &SelectWordLeft, _: &mut Window, cx: &mut Context<Self>) {
        self.select_to(self.word_from(self.cursor_offset(), true), cx);
    }

    fn select_word_right(&mut self, _: &SelectWordRight, _: &mut Window, cx: &mut Context<Self>) {
        self.select_to(self.word_from(self.cursor_offset(), false), cx);
    }

    fn select_all(&mut self, _: &SelectAll, _: &mut Window, cx: &mut Context<Self>) {
        self.move_to(0, cx);
        self.select_to(self.content.len(), cx);
    }

    fn home(&mut self, _: &Home, _: &mut Window, cx: &mut Context<Self>) {
        self.move_to(0, cx);
    }

    fn end(&mut self, _: &End, _: &mut Window, cx: &mut Context<Self>) {
        if self.hint_shown().is_some() {
            cx.emit(HintTaken);
        } else {
            self.move_to(self.content.len(), cx);
        }
    }

    fn select_home(&mut self, _: &SelectHome, _: &mut Window, cx: &mut Context<Self>) {
        self.select_to(0, cx);
    }

    fn select_end(&mut self, _: &SelectEnd, _: &mut Window, cx: &mut Context<Self>) {
        self.select_to(self.content.len(), cx);
    }

    fn backspace(&mut self, _: &Backspace, window: &mut Window, cx: &mut Context<Self>) {
        if self.at_start() {
            cx.emit(AtStart::Backspace);
            return;
        }
        if self.selected_range.is_empty() {
            self.select_to(self.previous_boundary(self.cursor_offset()), cx);
        }
        self.replace_text_in_range(None, "", window, cx);
    }

    fn delete(&mut self, _: &Delete, window: &mut Window, cx: &mut Context<Self>) {
        if self.selected_range.is_empty() {
            self.select_to(self.next_boundary(self.cursor_offset()), cx);
        }
        self.replace_text_in_range(None, "", window, cx);
    }

    fn delete_word_left(
        &mut self,
        _: &DeleteWordLeft,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.selected_range.is_empty() {
            self.select_to(self.previous_word(self.cursor_offset()), cx);
        }
        self.replace_text_in_range(None, "", window, cx);
    }

    fn delete_word_right(
        &mut self,
        _: &DeleteWordRight,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.selected_range.is_empty() {
            self.select_to(self.next_word(self.cursor_offset()), cx);
        }
        self.replace_text_in_range(None, "", window, cx);
    }

    fn undo(&mut self, _: &Undo, _: &mut Window, cx: &mut Context<Self>) {
        let current = (self.content.clone(), self.selected_range.clone());
        let Some((previous, together)) = self.history.undo(current) else {
            return;
        };
        self.restore(previous, cx);
        // An edit of several fields is taken back in each.
        for other in together
            .iter()
            .flat_map(|t| t.with.iter().map(move |w| (t.id, w)))
        {
            let (id, other) = other;
            let _ = other.update(cx, |other, cx| {
                let current = (other.content.clone(), other.selected_range.clone());
                if let Some(previous) = other.history.undo_together(id, current) {
                    other.restore(previous, cx);
                }
            });
        }
    }

    fn redo(&mut self, _: &Redo, _: &mut Window, cx: &mut Context<Self>) {
        let current = (self.content.clone(), self.selected_range.clone());
        let Some((next, together)) = self.history.redo(current) else {
            return;
        };
        self.restore(next, cx);
        // And put back in each.
        for other in together
            .iter()
            .flat_map(|t| t.with.iter().map(move |w| (t.id, w)))
        {
            let (id, other) = other;
            let _ = other.update(cx, |other, cx| {
                let current = (other.content.clone(), other.selected_range.clone());
                if let Some(next) = other.history.redo_together(id, current) {
                    other.restore(next, cx);
                }
            });
        }
    }

    fn restore(&mut self, (content, selected): Snapshot, cx: &mut Context<Self>) {
        self.content = content;
        self.styles.clear();
        self.selected_range = selected;
        self.selection_reversed = false;
        self.marked_range = None;
        cx.emit(InputEvent::Changed);
        cx.notify();
    }

    fn paste(&mut self, _: &Paste, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(text) = cx.read_from_clipboard().and_then(|item| item.text()) {
            self.replace_text_in_range(None, &text.replace('\n', " "), window, cx);
        }
    }

    fn copy(&mut self, _: &Copy, _: &mut Window, cx: &mut Context<Self>) {
        if !self.selected_range.is_empty() && !self.masked {
            cx.write_to_clipboard(ClipboardItem::new_string(
                self.content[self.selected_range.clone()].to_string(),
            ));
        }
    }

    fn cut(&mut self, _: &Cut, window: &mut Window, cx: &mut Context<Self>) {
        if !self.selected_range.is_empty() && !self.masked {
            cx.write_to_clipboard(ClipboardItem::new_string(
                self.content[self.selected_range.clone()].to_string(),
            ));
            self.replace_text_in_range(None, "", window, cx);
        }
    }

    fn submit(&mut self, _: &Submit, _: &mut Window, cx: &mut Context<Self>) {
        cx.emit(InputEvent::Submit);
    }

    fn cancel(&mut self, _: &Cancel, _: &mut Window, cx: &mut Context<Self>) {
        cx.emit(InputEvent::Cancel);
    }

    // ------------------------------------------------------------ mouse

    /// A press: one places the caret (Shift: takes the selection there), a
    /// double one selects the word under it (or the spaces, or the
    /// punctuation), and a drag from it goes on a word at a time; a triple
    /// one selects the whole text.
    fn on_mouse_down(&mut self, ev: &MouseDownEvent, _: &mut Window, cx: &mut Context<Self>) {
        self.is_selecting = true;
        self.selecting_words = None;
        if ev.click_count >= 3 {
            self.select_all_now(cx);
        } else if ev.click_count == 2 {
            let word = self.word_under(ev.position);
            self.selected_range = word.clone();
            self.selection_reversed = false;
            self.selecting_words = Some(word);
            cx.notify();
        } else if ev.modifiers.shift {
            self.select_to(self.index_for_mouse_position(ev.position), cx);
        } else {
            self.move_to(self.index_for_mouse_position(ev.position), cx);
        }
    }

    fn on_mouse_up(&mut self, _: &MouseUpEvent, _: &mut Window, _: &mut Context<Self>) {
        self.stop_selecting();
    }

    fn stop_selecting(&mut self) {
        self.is_selecting = false;
        self.selecting_words = None;
        self.dragged_at = None;
        self.autoscroll = None;
    }

    /// The segment of the text drawn under `position` (`segment::segment_at`):
    /// the character under it, as the line is drawn in either direction, or,
    /// past an end, the one nearest. A secret's whole text: its words are not
    /// told.
    fn word_under(&self, position: Point<Pixels>) -> Range<usize> {
        let (Some(bounds), Some(line)) = (self.last_bounds.as_ref(), self.last_layout.as_ref())
        else {
            return 0..0;
        };
        if self.masked || self.content.is_empty() {
            return 0..self.content.len();
        }
        let x = position.x - bounds.left() + self.scroll_x;
        let at = line
            .index_for_x(x)
            .unwrap_or_else(|| line.closest_index_for_x(x));
        segment::segment_at(&self.content, self.text_offset(at))
    }

    /// The selection a drag takes to `offset`: there, or, from a double
    /// click, a word at a time, the word clicked always in it (`words`).
    fn drag_select(&mut self, offset: usize, cx: &mut Context<Self>) {
        match &self.selecting_words {
            Some(word) => {
                let (range, reversed) = by_words(&self.content, word, offset);
                self.selected_range = range;
                self.selection_reversed = reversed;
                cx.notify();
            }
            None => self.select_to(offset, cx),
        }
    }

    /// The pointer moved to `position` while a drag selects, inside the
    /// field or out of it: the selection goes to where along the line it
    /// is, beyond what is shown when it is past an end, and goes on there
    /// while it is held (`autoscroll_step`).
    fn drag_to(&mut self, position: Point<Pixels>, cx: &mut Context<Self>) {
        if !self.is_selecting {
            return;
        }
        self.dragged_at = Some(position);
        self.drag_select(self.index_for_mouse_position(position), cx);
        let outside = self
            .last_bounds
            .is_some_and(|bounds| position.x < bounds.left() || position.x > bounds.right());
        if outside && self.autoscroll.is_none() {
            self.autoscroll = Some(cx.spawn(async move |input, cx| {
                loop {
                    cx.background_executor().timer(AUTOSCROLL_EVERY).await;
                    let goes_on = input.update(cx, Self::autoscroll_step).unwrap_or(false);
                    if !goes_on {
                        break;
                    }
                }
            }));
        }
    }

    /// One step of a drag held past an end of the field: the selection
    /// goes on a little past what is shown, and the line scrolls to show
    /// where it ends (`scrolled`, in prepaint). Whether to go on.
    fn autoscroll_step(&mut self, cx: &mut Context<Self>) -> bool {
        let reached = match (
            self.is_selecting,
            self.dragged_at,
            self.last_bounds,
            self.last_layout.as_ref(),
        ) {
            (true, Some(at), Some(bounds), Some(line)) => {
                autoscroll_x(at.x, bounds.left(), bounds.right(), self.scroll_x)
                    .map(|x| self.text_offset(line.closest_index_for_x(x)))
            }
            _ => None,
        };
        let Some(offset) = reached else {
            self.autoscroll = None;
            return false;
        };
        let before = self.selected_range.clone();
        self.drag_select(offset, cx);
        // At an end of the text: nothing more to show.
        if self.selected_range == before {
            self.autoscroll = None;
            return false;
        }
        true
    }

    // ------------------------------------------------------------ offsets

    fn move_to(&mut self, offset: usize, cx: &mut Context<Self>) {
        self.selected_range = offset..offset;
        cx.notify();
    }

    fn cursor_offset(&self) -> usize {
        if self.selection_reversed {
            self.selected_range.start
        } else {
            self.selected_range.end
        }
    }

    /// The offset nearest `position` along the line: one line, so only
    /// where along it counts, and a drag that strays above or below the
    /// field goes on selecting along it. Past an end of what is shown, the
    /// offset there in the text, which the line scrolls to.
    fn index_for_mouse_position(&self, position: Point<Pixels>) -> usize {
        if self.content.is_empty() {
            return 0;
        }
        let (Some(bounds), Some(line)) = (self.last_bounds.as_ref(), self.last_layout.as_ref())
        else {
            return 0;
        };
        self.text_offset(line.closest_index_for_x(position.x - bounds.left() + self.scroll_x))
    }

    fn select_to(&mut self, offset: usize, cx: &mut Context<Self>) {
        if self.selection_reversed {
            self.selected_range.start = offset;
        } else {
            self.selected_range.end = offset;
        }
        if self.selected_range.end < self.selected_range.start {
            self.selection_reversed = !self.selection_reversed;
            self.selected_range = self.selected_range.end..self.selected_range.start;
        }
        cx.notify();
    }

    fn offset_from_utf16(&self, offset: usize) -> usize {
        let mut utf8_offset = 0;
        let mut utf16_count = 0;
        for ch in self.content.chars() {
            if utf16_count >= offset {
                break;
            }
            utf16_count += ch.len_utf16();
            utf8_offset += ch.len_utf8();
        }
        utf8_offset
    }

    fn offset_to_utf16(&self, offset: usize) -> usize {
        let mut utf16_offset = 0;
        let mut utf8_count = 0;
        for ch in self.content.chars() {
            if utf8_count >= offset {
                break;
            }
            utf8_count += ch.len_utf8();
            utf16_offset += ch.len_utf16();
        }
        utf16_offset
    }

    fn range_to_utf16(&self, range: &Range<usize>) -> Range<usize> {
        self.offset_to_utf16(range.start)..self.offset_to_utf16(range.end)
    }

    fn range_from_utf16(&self, range_utf16: &Range<usize>) -> Range<usize> {
        self.offset_from_utf16(range_utf16.start)..self.offset_from_utf16(range_utf16.end)
    }

    /// The line as last drawn, where it still shows the text and some of
    /// it reads right to left: what the arrows follow as it is drawn. A
    /// secret's bullets read left to right.
    fn drawn_bidi(&self) -> Option<&ShapedLine> {
        self.last_layout.as_ref().filter(|line| {
            !self.masked
                && line.has_right_to_left()
                && line.text.get(..self.content.len()) == Some(self.content.as_ref())
        })
    }

    /// The place of the text next to `offset` on its left or its right as
    /// the line is drawn, passing places inside a character: where some of
    /// it reads right to left (`ShapedLine::index_left_of`). `None` at that
    /// end of the line, or where all of it reads left to right.
    fn drawn_beside(&self, offset: usize, toward_left: bool) -> Option<usize> {
        let line = self.drawn_bidi()?;
        let characters: std::collections::HashSet<usize> = self
            .content
            .grapheme_indices(true)
            .map(|(at, _)| at)
            .chain([self.content.len()])
            .collect();
        let mut at = offset;
        // A step a place at most: never more than the text has.
        for _ in 0..=self.content.len() {
            at = if toward_left {
                line.index_left_of(at)?
            } else {
                line.index_right_of(at)?
            };
            if at > self.content.len() {
                return None;
            }
            if characters.contains(&at) {
                return Some(at);
            }
        }
        None
    }

    /// Where an arrow moves the caret from `offset`: the character on its
    /// left or right as the line is drawn, so Left goes left in Hebrew too;
    /// the previous or next one where all of it reads left to right. At an
    /// end, where it is.
    fn arrow_from(&self, offset: usize, toward_left: bool) -> usize {
        if self.drawn_bidi().is_some() {
            return self.drawn_beside(offset, toward_left).unwrap_or(offset);
        }
        if toward_left {
            self.previous_boundary(offset)
        } else {
            self.next_boundary(offset)
        }
    }

    /// Where Ctrl and an arrow move the caret from `offset`: the previous
    /// or next word in the text's order, the other way round where the line
    /// reads right to left, whose next word is on its left (`word_toward`).
    fn word_from(&self, offset: usize, toward_left: bool) -> usize {
        // A secret's bullets read left to right.
        if self.masked {
            return if toward_left { 0 } else { self.content.len() };
        }
        let right_to_left = self
            .drawn_bidi()
            .is_some_and(|line| line.is_right_to_left());
        word_toward(&self.content, offset, toward_left, right_to_left)
    }

    /// The end of the selection drawn on the left (or the right): its
    /// start, or its end where the line reads right to left there.
    fn selection_end_on(&self, left: bool) -> usize {
        let range = self.selected_range.clone();
        let Some(line) = self.drawn_bidi() else {
            return if left { range.start } else { range.end };
        };
        let (start, end) = (line.x_for_index(range.start), line.x_for_index(range.end));
        if (start <= end) == left {
            range.start
        } else {
            range.end
        }
    }

    fn previous_boundary(&self, offset: usize) -> usize {
        self.content
            .grapheme_indices(true)
            .rev()
            .find_map(|(idx, _)| (idx < offset).then_some(idx))
            .unwrap_or(0)
    }

    fn next_boundary(&self, offset: usize) -> usize {
        self.content
            .grapheme_indices(true)
            .find_map(|(idx, _)| (idx > offset).then_some(idx))
            .unwrap_or(self.content.len())
    }

    /// The start of the word `offset` is in or after, in the text's order
    /// (Ctrl+Backspace); a secret's start.
    fn previous_word(&self, offset: usize) -> usize {
        if self.masked {
            return 0;
        }
        segment::word_start_before(&self.content, offset)
    }

    /// The end of the word `offset` is in or before (Ctrl+Delete); a
    /// secret's end.
    fn next_word(&self, offset: usize) -> usize {
        if self.masked {
            return self.content.len();
        }
        segment::word_end_after(&self.content, offset)
    }
}

/// Where Ctrl and the arrow toward the left (or the right) move the caret
/// from `offset` in `text`: the start of the word it is in or after, or the
/// end of the word it is in or before, in the text's order; the other way
/// round where the line reads right to left, its next word on its left.
fn word_toward(text: &str, offset: usize, toward_left: bool, right_to_left: bool) -> usize {
    if toward_left == right_to_left {
        segment::word_end_after(text, offset)
    } else {
        segment::word_start_before(text, offset)
    }
}

/// The selection a drag from a double click on `word` takes to the caret
/// place `offset` in `text`: the word, and each segment from it to the one
/// `offset` is in, whole; and whether it is reversed (the drag went back,
/// its moving end the start).
fn by_words(text: &str, word: &Range<usize>, offset: usize) -> (Range<usize>, bool) {
    if offset < word.start {
        (segment::segment_at(text, offset).start..word.end, true)
    } else if offset > word.end {
        // The segment of the character before the place.
        (word.start..segment::segment_at(text, offset - 1).end, false)
    } else {
        (word.clone(), false)
    }
}

impl EntityInputHandler for TextInput {
    fn text_for_range(
        &mut self,
        range_utf16: Range<usize>,
        actual_range: &mut Option<Range<usize>>,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> Option<String> {
        let range = self.range_from_utf16(&range_utf16);
        actual_range.replace(self.range_to_utf16(&range));
        Some(self.content[range].to_string())
    }

    fn selected_text_range(
        &mut self,
        _ignore_disabled_input: bool,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> Option<UTF16Selection> {
        Some(UTF16Selection {
            range: self.range_to_utf16(&self.selected_range),
            reversed: self.selection_reversed,
        })
    }

    fn marked_text_range(&self, _: &mut Window, _: &mut Context<Self>) -> Option<Range<usize>> {
        self.marked_range
            .as_ref()
            .map(|range| self.range_to_utf16(range))
    }

    fn unmark_text(&mut self, _: &mut Window, _: &mut Context<Self>) {
        self.marked_range = None;
    }

    fn replace_text_in_range(
        &mut self,
        range_utf16: Option<Range<usize>>,
        new_text: &str,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let range = range_utf16
            .as_ref()
            .map(|r| self.range_from_utf16(r))
            .or(self.marked_range.clone())
            .unwrap_or(self.selected_range.clone());
        let new_text = fitted(&self.content, &range, new_text);
        if changes_nothing(&range, new_text, self.marked_range.is_some()) {
            return;
        }
        // A composition being committed was recorded when it began.
        if self.marked_range.is_none() && !self.masked {
            let before = (self.content.clone(), self.selected_range.clone());
            self.history.record(before, typed_at(&range, new_text));
        }
        self.content =
            (self.content[0..range.start].to_owned() + new_text + &self.content[range.end..])
                .into();
        kept_before(&mut self.styles, range.start);
        self.selected_range = range.start + new_text.len()..range.start + new_text.len();
        self.marked_range.take();
        cx.emit(InputEvent::Changed);
        cx.notify();
    }

    fn replace_and_mark_text_in_range(
        &mut self,
        range_utf16: Option<Range<usize>>,
        new_text: &str,
        new_selected_range_utf16: Option<Range<usize>>,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let range = range_utf16
            .as_ref()
            .map(|r| self.range_from_utf16(r))
            .or(self.marked_range.clone())
            .unwrap_or(self.selected_range.clone());
        let whole = new_text;
        let new_text = fitted(&self.content, &range, new_text);
        if changes_nothing(&range, new_text, self.marked_range.is_some()) {
            return;
        }
        // A selection given within a composition cut short is past its end.
        let new_selected_range_utf16 = new_selected_range_utf16.filter(|_| new_text == whole);
        // A composition is one step, from the text before it began.
        if self.marked_range.is_none() && !self.masked {
            let before = (self.content.clone(), self.selected_range.clone());
            self.history.record(before, None);
        }
        self.content =
            (self.content[0..range.start].to_owned() + new_text + &self.content[range.end..])
                .into();
        kept_before(&mut self.styles, range.start);
        self.marked_range = if new_text.is_empty() {
            None
        } else {
            Some(range.start..range.start + new_text.len())
        };
        // The new selection is given in UTF-16 within `new_text`, which now
        // starts at `range.start`: both ends move by that much.
        self.selected_range = new_selected_range_utf16.as_ref().map_or_else(
            || range.start + new_text.len()..range.start + new_text.len(),
            |r| {
                let start = range.start + utf16_to_utf8(new_text, r.start);
                let end = range.start + utf16_to_utf8(new_text, r.end);
                start.min(self.content.len())..end.min(self.content.len())
            },
        );
        cx.emit(InputEvent::Changed);
        cx.notify();
    }

    fn bounds_for_range(
        &mut self,
        range_utf16: Range<usize>,
        bounds: Bounds<Pixels>,
        _: &mut Window,
        _: &mut Context<Self>,
    ) -> Option<Bounds<Pixels>> {
        let last_layout = self.last_layout.as_ref()?;
        let range = self.range_from_utf16(&range_utf16);
        // The layout is of what is shown: bullets, when masked. Where the
        // text turns direction, a range is drawn in more than one place: it
        // is said from the leftmost to the rightmost.
        let left = bounds.left() - self.scroll_x;
        let shown = self.shown_offset(range.start)..self.shown_offset(range.end);
        let spans = last_layout.selection_ranges(shown.clone());
        let (from, to) = if let (Some(first), Some(last)) = (spans.first(), spans.last()) {
            (first.start, last.end)
        } else {
            let at = last_layout.x_for_index(shown.start);
            (at, at)
        };
        Some(Bounds::from_corners(
            point(left + from, bounds.top()),
            point(left + to, bounds.bottom()),
        ))
    }

    fn character_index_for_point(
        &mut self,
        point: Point<Pixels>,
        _: &mut Window,
        _: &mut Context<Self>,
    ) -> Option<usize> {
        let line_point = self.last_bounds?.localize(&point)?;
        let last_layout = self.last_layout.as_ref()?;
        let shown = last_layout.index_for_x(line_point.x + self.scroll_x)?;
        Some(self.offset_to_utf16(self.text_offset(shown)))
    }
}

struct TextElement {
    input: Entity<TextInput>,
}

struct PrepaintState {
    line: Option<ShapedLine>,
    cursor: Option<PaintQuad>,
    /// One for each stretch of the selection drawn side by side: more than
    /// one where the text turns direction.
    selection: Vec<PaintQuad>,
    scroll_x: Pixels,
}

/// How often a drag held past an end of a field selects on.
const AUTOSCROLL_EVERY: std::time::Duration = std::time::Duration::from_millis(30);

/// Where along the line a drag held at `pointer`, past an end of a field
/// from `left` to `right` scrolled by `scroll`, selects to at its next
/// step: as far past what is shown as the pointer is past the field,
/// between 2 and 40 px a step, so the further it is held the faster the
/// line goes. `None` while the pointer is over the field.
fn autoscroll_x(pointer: Pixels, left: Pixels, right: Pixels, scroll: Pixels) -> Option<Pixels> {
    let step = |beyond: Pixels| beyond.clamp(px(2.), px(40.));
    if pointer > right {
        Some(scroll + (right - left) + step(pointer - right))
    } else if pointer < left {
        Some((scroll - step(left - pointer)).max(px(0.)))
    } else {
        None
    }
}

/// How far to scroll a line `width` wide in a field `field` wide, from
/// `scroll`, so the cursor at `cursor` (and its 2 px bar) stays in view:
/// no more than needed, never past either end, and none when it fits. The
/// scroll is the line's x drawn at the field's left edge. A line that fits
/// stands at the field's start: its left, or its right where the field is
/// laid out right to left (`from_right`, the window mirrored: `sides`), the
/// line then drawn from a negative scroll, its end and the cursor's bar
/// against the right edge.
fn scrolled(
    scroll: Pixels,
    width: Pixels,
    field: Pixels,
    cursor: Pixels,
    from_right: bool,
) -> Pixels {
    let bar = px(2.);
    if width + bar <= field {
        return if from_right {
            width + bar - field
        } else {
            px(0.)
        };
    }
    let scroll = if cursor < scroll {
        cursor
    } else if cursor + bar > scroll + field {
        cursor + bar - field
    } else {
        scroll
    };
    scroll.clamp(px(0.), width + bar - field)
}

impl IntoElement for TextElement {
    type Element = Self;

    fn into_element(self) -> Self::Element {
        self
    }
}

impl Element for TextElement {
    type RequestLayoutState = ();
    type PrepaintState = PrepaintState;

    fn id(&self) -> Option<ElementId> {
        None
    }

    fn source_location(&self) -> Option<&'static core::panic::Location<'static>> {
        None
    }

    fn request_layout(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        window: &mut Window,
        cx: &mut App,
    ) -> (LayoutId, Self::RequestLayoutState) {
        let mut style = Style::default();
        style.size.width = relative(1.).into();
        style.size.height = window.line_height().into();
        (window.request_layout(style, [], cx), ())
    }

    fn prepaint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        _: &mut Self::RequestLayoutState,
        window: &mut Window,
        cx: &mut App,
    ) -> Self::PrepaintState {
        let theme = cx.global::<Theme>().clone();
        let input = self.input.read(cx);
        // A masked input shapes bullets; offsets into the text become
        // offsets into the bullets (one bullet per character).
        let masked = input.masked;
        let shown = |offset: usize| input.shown_offset(offset);
        let content: SharedString = if masked {
            BULLET
                .to_string()
                .repeat(input.content.chars().count())
                .into()
        } else {
            input.content.clone()
        };
        let selected_range = shown(input.selected_range.start)..shown(input.selected_range.end);
        let cursor = shown(input.cursor_offset());
        let marked_range = input
            .marked_range
            .as_ref()
            .map(|marked| shown(marked.start)..shown(marked.end));
        let style = window.text_style();

        let (display_text, text_color) = if content.is_empty() {
            (
                SharedString::from(input.placeholder.say()),
                theme.text_muted,
            )
        } else {
            (content, style.color)
        };
        // A hint follows the text, muted, only where it shows.
        let hint = input
            .hint_shown()
            .filter(|_| !input.content.is_empty() && input.focus_handle.is_focused(window))
            .cloned();
        let typed_len = display_text.len();
        let display_text: SharedString = match &hint {
            Some(hint) => format!("{display_text}{hint}").into(),
            None => display_text,
        };

        let run = TextRun {
            len: typed_len,
            font: style.font(),
            color: text_color,
            background_color: None,
            underline: None,
            strikethrough: None,
            position: gpui::RunPosition::Baseline,
        };
        let runs = if let Some(marked) = marked_range.as_ref() {
            vec![
                TextRun {
                    len: marked.start,
                    ..run.clone()
                },
                TextRun {
                    len: marked.end - marked.start,
                    underline: Some(UnderlineStyle {
                        color: Some(run.color),
                        thickness: px(1.0),
                        wavy: false,
                    }),
                    ..run.clone()
                },
                TextRun {
                    len: typed_len - marked.end,
                    ..run
                },
            ]
            .into_iter()
            .filter(|r| r.len > 0)
            .collect()
        } else {
            let mut runs = if masked || input.content.is_empty() || input.styles.is_empty() {
                vec![run.clone()]
            } else {
                styled_runs(&run, &input.content, &input.styles)
            };
            if hint.is_some() {
                runs.push(TextRun {
                    len: display_text.len() - typed_len,
                    color: theme.text_muted,
                    background_color: None,
                    underline: None,
                    ..run
                });
            }
            runs
        };

        // What is selected is written in the selection's own text color, on
        // the selection painted under it (`Chosen`).
        let runs = selected_runs(runs, &selected_range, theme.selection_text);
        let font_size = style.font_size.to_pixels(window.rem_size());
        let line = window
            .text_system()
            .shape_line(display_text, font_size, &runs, None);

        let cursor_pos = line.x_for_index(cursor);
        // Laid out right to left (the window mirrored, `sides`), the line
        // stands at the field's right, its start, the placeholder too.
        let from_right = window.layout_direction().is_rtl();
        // Unfocused, a long text shows its start, as a label would: where
        // its first character is drawn (its right end for a text read right
        // to left), from the field's start.
        let scroll_x = if input.focus_handle.is_focused(window) {
            scrolled(
                input.scroll_x,
                line.width,
                bounds.size.width,
                cursor_pos,
                from_right,
            )
        } else if from_right {
            let start = line.width + px(2.) - bounds.size.width;
            scrolled(
                start,
                line.width,
                bounds.size.width,
                line.x_for_index(0),
                from_right,
            )
        } else {
            px(0.)
        };
        let left = bounds.left() - scroll_x;
        let (selection, cursor) = if selected_range.is_empty() {
            (
                Vec::new(),
                Some(fill(
                    Bounds::new(
                        point(left + cursor_pos, bounds.top()),
                        size(px(2.), bounds.bottom() - bounds.top()),
                    ),
                    theme.accent,
                )),
            )
        } else {
            (
                line.selection_ranges(selected_range)
                    .into_iter()
                    .map(|span| {
                        fill(
                            Bounds::from_corners(
                                point(left + span.start, bounds.top()),
                                point(left + span.end, bounds.bottom()),
                            ),
                            theme.selection_bg,
                        )
                    })
                    .collect(),
                None,
            )
        };
        PrepaintState {
            line: Some(line),
            cursor,
            selection,
            scroll_x,
        }
    }

    fn paint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        _: &mut Self::RequestLayoutState,
        prepaint: &mut Self::PrepaintState,
        window: &mut Window,
        cx: &mut App,
    ) {
        let focus_handle = self.input.read(cx).focus_handle.clone();
        window.handle_input(
            &focus_handle,
            ElementInputHandler::new(bounds, self.input.clone()),
            cx,
        );
        let line = prepaint.line.take().unwrap();
        let scroll_x = prepaint.scroll_x;
        // A line longer than the field is clipped to it, never drawn over
        // what sits beside the field.
        window.with_content_mask(Some(gpui::ContentMask::new(bounds)), |window| {
            for selection in prepaint.selection.drain(..) {
                window.paint_quad(selection);
            }
            let origin = point(bounds.left() - scroll_x, bounds.top());
            line.paint(
                origin,
                window.line_height(),
                gpui::TextAlign::Left,
                None,
                window,
                cx,
            )
            .unwrap();
            if focus_handle.is_focused(window)
                && let Some(cursor) = prepaint.cursor.take()
            {
                window.paint_quad(cursor);
            }
        });
        let selecting = self.input.update(cx, |input, _| {
            input.last_layout = Some(line);
            input.last_bounds = Some(bounds);
            input.scroll_x = scroll_x;
            input.is_selecting
        });
        // A drag that selects is followed wherever it goes in the window,
        // not only over the field: past an end, the text beyond is
        // selected and scrolled to (`TextInput::drag_to`).
        if selecting {
            let input = self.input.clone();
            window.on_mouse_event(move |event: &MouseMoveEvent, phase, _, cx| {
                if phase != gpui::DispatchPhase::Bubble {
                    return;
                }
                // A move said without its button (Wayland's on coming back
                // into the window) is not the drag's end: the button's
                // release is (`on_mouse_up_out`), which the window hears
                // wherever the pointer is, holding it while pressed.
                if event.pressed_button == Some(MouseButton::Left) {
                    input.update(cx, |input, cx| input.drag_to(event.position, cx));
                }
            });
        }
    }
}

impl TextInput {
    /// The field as assistive technology reads it: named, its text one run
    /// it can walk unit by unit, the caret and selection in those units. A
    /// secret says bullets, as the screen shows it.
    fn expose(&self, el: gpui::Stateful<gpui::Div>) -> gpui::Stateful<gpui::Div> {
        let shown: String = if self.masked {
            std::iter::repeat_n(BULLET, self.content.chars().count()).collect()
        } else {
            self.content.to_string()
        };
        let lengths = units(&shown);
        // A secret's words are not told: its run is one.
        let word_starts = (!self.masked)
            .then(|| word_starts(&shown, &lengths))
            .flatten();
        let (anchor, focus) = if self.selection_reversed {
            (self.selected_range.end, self.selected_range.start)
        } else {
            (self.selected_range.start, self.selected_range.end)
        };
        let anchor = unit_at(&lengths, self.shown_offset(anchor));
        let focus = unit_at(&lengths, self.shown_offset(focus));
        let named = self.label.is_some();
        el.aria_label(SharedString::from(
            self.label.as_ref().unwrap_or(&self.placeholder).say(),
        ))
        .when(named && !self.placeholder.is_empty(), |d| {
            d.aria_placeholder(SharedString::from(self.placeholder.say()))
        })
        .aria_value(SharedString::from(shown.clone()))
        .a11y_synthetic_children(move |builder| {
            let run_id = builder.synthetic_node_id(0);
            let mut run = gpui::accesskit::Node::new(gpui::Role::TextRun);
            run.set_value(shown);
            run.set_character_lengths(lengths);
            if let Some(starts) = word_starts {
                run.set_word_starts(starts);
            }
            builder.push_child(run_id, run);
            let at = |character_index| gpui::accesskit::TextPosition {
                node: run_id,
                character_index,
            };
            builder
                .parent_node()
                .set_text_selection(gpui::accesskit::TextSelection {
                    anchor: at(anchor),
                    focus: at(focus),
                });
        })
    }
}

/// The smallest units the caret moves by in `text` (its graphemes), each's
/// length in bytes, as AccessKit counts a text's characters. A unit longer
/// than a byte can count is split into its chars.
fn units(text: &str) -> Vec<u8> {
    let mut out = Vec::with_capacity(text.len());
    for grapheme in text.graphemes(true) {
        match u8::try_from(grapheme.len()) {
            Ok(len) => out.push(len),
            Err(_) => out.extend(
                grapheme
                    .chars()
                    .map(|c| u8::try_from(c.len_utf8()).unwrap_or(4)),
            ),
        }
    }
    out
}

/// The unit each word of `text` starts at (`units`' `lengths`), as AccessKit
/// reads a run's words: a screen reader moving by words in the field stops
/// where Ctrl and the arrows do, in any script (`segment::words`). `None`
/// where a word starts past what AccessKit counts (255 units): the run is
/// then read as one word rather than with its words wrong.
fn word_starts(text: &str, lengths: &[u8]) -> Option<Vec<u8>> {
    let (mut unit, mut at) = (0, 0);
    let mut starts = Vec::new();
    for (start, _) in segment::words(text) {
        while at < start && unit < lengths.len() {
            at += usize::from(lengths[unit]);
            unit += 1;
        }
        starts.push(u8::try_from(unit).ok()?);
    }
    Some(starts)
}

/// The unit that byte `offset` starts, or the end.
fn unit_at(lengths: &[u8], offset: usize) -> usize {
    let mut at = 0;
    for (index, len) in lengths.iter().enumerate() {
        if at >= offset {
            return index;
        }
        at += usize::from(*len);
    }
    lengths.len()
}

impl Render for TextInput {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let a11y = window.is_a11y_active();
        div()
            .id(("text-input", cx.entity_id().as_u64() as usize))
            .role(if self.masked {
                gpui::Role::PasswordInput
            } else {
                gpui::Role::TextInput
            })
            .when(a11y, |d| self.expose(d))
            .flex()
            .w_full()
            .key_context(KEY_CONTEXT)
            .track_focus(&self.focus_handle(cx))
            .cursor(CursorStyle::IBeam)
            .on_action(cx.listener(Self::backspace))
            .on_action(cx.listener(Self::delete))
            .on_action(cx.listener(Self::left))
            .on_action(cx.listener(Self::right))
            .on_action(cx.listener(Self::select_left))
            .on_action(cx.listener(Self::select_right))
            .on_action(cx.listener(Self::word_left))
            .on_action(cx.listener(Self::word_right))
            .on_action(cx.listener(Self::select_word_left))
            .on_action(cx.listener(Self::select_word_right))
            .on_action(cx.listener(Self::select_all))
            .on_action(cx.listener(Self::home))
            .on_action(cx.listener(Self::end))
            .on_action(cx.listener(Self::select_home))
            .on_action(cx.listener(Self::select_end))
            .on_action(cx.listener(Self::paste))
            .on_action(cx.listener(Self::cut))
            .on_action(cx.listener(Self::copy))
            .on_action(cx.listener(Self::submit))
            .on_action(cx.listener(Self::cancel))
            .on_action(cx.listener(Self::undo))
            .on_action(cx.listener(Self::redo))
            .on_action(cx.listener(Self::delete_word_left))
            .on_action(cx.listener(Self::delete_word_right))
            .on_mouse_down(MouseButton::Left, cx.listener(Self::on_mouse_down))
            .on_mouse_up(MouseButton::Left, cx.listener(Self::on_mouse_up))
            .on_mouse_up_out(MouseButton::Left, cx.listener(Self::on_mouse_up))
            .child(TextElement { input: cx.entity() })
    }
}

impl Focusable for TextInput {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

/// `offset` within `text`, at the start of the character it falls in.
fn snapped(text: &str, offset: usize) -> usize {
    let mut offset = offset.min(text.len());
    while !text.is_char_boundary(offset) {
        offset -= 1;
    }
    offset
}

/// What an edit at `at` leaves of the styles: those wholly before it.
fn kept_before(styles: &mut Vec<(Range<usize>, Paint)>, at: usize) {
    styles.retain(|(range, _)| range.end <= at);
}

/// `run` cut where `styles` draw parts of `text` otherwise. The styles are
/// taken in order of their start; one that overlaps the one before, or
/// does not fall on character boundaries, is left out.
/// `runs` with what lies in `range` (bytes of the text they cover) written
/// as selected: in `color`, and on nothing but the selection painted under
/// it (a term's tint would hide it).
fn selected_runs(runs: Vec<TextRun>, range: &Range<usize>, color: gpui::Hsla) -> Vec<TextRun> {
    if range.is_empty() {
        return runs;
    }
    let mut out = Vec::with_capacity(runs.len() + 2);
    let mut at = 0;
    for run in runs {
        let (start, end) = (at, at + run.len);
        at = end;
        // Its parts before the range, within it, after it.
        let cuts = [
            start,
            range.start.clamp(start, end),
            range.end.clamp(start, end),
            end,
        ];
        for (part, ends) in cuts.windows(2).enumerate() {
            if ends[1] > ends[0] {
                let mut piece = TextRun {
                    len: ends[1] - ends[0],
                    ..run.clone()
                };
                if part == 1 {
                    piece.color = color;
                    piece.background_color = None;
                }
                out.push(piece);
            }
        }
    }
    out
}

fn styled_runs(run: &TextRun, text: &str, styles: &[(Range<usize>, Paint)]) -> Vec<TextRun> {
    let mut sorted: Vec<&(Range<usize>, Paint)> = styles.iter().collect();
    sorted.sort_by_key(|(range, _)| range.start);
    let mut runs = Vec::new();
    let mut at = 0;
    for (range, paint) in sorted {
        if range.start < at
            || range.end > text.len()
            || range.is_empty()
            || !text.is_char_boundary(range.start)
            || !text.is_char_boundary(range.end)
        {
            continue;
        }
        if range.start > at {
            runs.push(TextRun {
                len: range.start - at,
                ..run.clone()
            });
        }
        runs.push(TextRun {
            len: range.end - range.start,
            color: paint.color.unwrap_or(run.color),
            background_color: paint.background,
            underline: paint.wavy.map(|color| UnderlineStyle {
                color: Some(color),
                thickness: px(1.0),
                wavy: true,
            }),
            ..run.clone()
        });
        at = range.end;
    }
    if at < text.len() {
        runs.push(TextRun {
            len: text.len() - at,
            ..run.clone()
        });
    }
    runs
}

#[cfg(test)]
mod tests {
    use super::{
        History, MOST_CHARS, Snapshot, Together, autoscroll_x, by_words, changes_nothing, fitted,
        held, scrolled, selected_runs, snapped, typed_at, unit_at, units, utf16_to_utf8,
        word_starts, word_toward,
    };

    /// Ctrl and the arrows go by a script's words, spaces or none: the next
    /// word's end toward the right, the previous one's start toward the
    /// left, as the text reads.
    #[test]
    fn ctrl_and_the_arrows_go_by_words_in_every_script() {
        let walk = |text: &str, toward_left: bool| {
            let mut at = if toward_left { text.len() } else { 0 };
            let mut stops = Vec::new();
            loop {
                let next = word_toward(text, at, toward_left, false);
                if next == at {
                    return stops;
                }
                stops.push(next);
                at = next;
            }
        };
        // Chinese: 我们|在|北京, each three bytes a character.
        assert_eq!(walk("我们在北京", false), [6, 9, 15]);
        assert_eq!(walk("我们在北京", true), [9, 6, 0]);
        // Japanese: 設定|を|開く.
        assert_eq!(walk("設定を開く", false), [6, 9, 15]);
        // Thai: เปิด|ไฟล์ (open, file), marks kept with their letters.
        assert_eq!(walk("เปิดไฟล์", false), ["เปิด".len(), "เปิดไฟล์".len()]);
        // French: an apostrophe joins, a space and guillemets part; past
        // the last word, the text's end.
        assert_eq!(walk("l'école « été »", false), [8, 17, 20]);
        assert_eq!(walk("l'école « été »", true), [12, 0]);
    }

    /// In a line read right to left, the next word is on the left: Ctrl
    /// and Left go on through the text, Ctrl and Right back.
    #[test]
    fn ctrl_and_the_arrows_are_swapped_where_the_line_reads_right_to_left() {
        let hebrew = "שלום עולם";
        let (shalom, olam) = ("שלום".len(), hebrew.find('ע').unwrap());
        assert_eq!(word_toward(hebrew, 0, true, true), shalom);
        assert_eq!(word_toward(hebrew, shalom, true, true), hebrew.len());
        assert_eq!(word_toward(hebrew, hebrew.len(), false, true), olam);
        assert_eq!(word_toward(hebrew, olam, false, true), 0);
        // Read left to right, the same line goes the other way.
        assert_eq!(word_toward(hebrew, 0, false, false), shalom);
    }

    /// A drag from a double click takes whole words, the one clicked kept.
    #[test]
    fn a_drag_from_a_double_click_goes_a_word_at_a_time() {
        let text = "copy 日本語の設定 now";
        let word = text.find('日').unwrap()..text.find('の').unwrap();
        assert_eq!(by_words(text, &word, word.start + 3), (word.clone(), false));
        // On into 設定: to its end; back into "copy": from its start.
        let settings = text.find('設').unwrap();
        assert_eq!(
            by_words(text, &word, settings + 3),
            (word.start..settings + 6, false)
        );
        assert_eq!(by_words(text, &word, 2), (0..word.end, true));
        // Past the end: to it.
        assert_eq!(
            by_words(text, &word, text.len()),
            (word.start..text.len(), false)
        );
    }

    /// A screen reader is told where each word starts, in units, in any
    /// script; nothing where a start is past what AccessKit counts.
    #[test]
    fn a_screen_reader_is_told_where_words_start() {
        let text = "Open 設定を開く now";
        assert_eq!(word_starts(text, &units(text)), Some(vec![0, 5, 7, 8, 11]));
        let long = "word ".repeat(60);
        assert_eq!(word_starts(&long, &units(&long)), None);
        assert_eq!(word_starts("", &[]), Some(vec![]));
    }

    /// A field takes what fits of a paste, whole characters, and all of
    /// what fits; text replaced makes room.
    #[test]
    fn a_field_takes_what_fits_of_a_long_paste_in_whole_graphemes() {
        // 20,000 paths pasted in an empty field: cut at the bound.
        let paths = "/tmp/many/f00000.txt ".repeat(20_000);
        let taken = fitted("", &(0..0), &paths);
        assert_eq!(taken.chars().count(), MOST_CHARS);
        assert!(paths.starts_with(taken));
        // Room left beside what stays; what is replaced counts for none.
        let content = "a".repeat(MOST_CHARS - 2);
        assert_eq!(fitted(&content, &(0..0), "bcd"), "bc");
        assert_eq!(fitted(&content, &(0..4), "bcdefg"), "bcdefg");
        // A grapheme of two characters never cut in two: é as e and its accent.
        let content = "a".repeat(MOST_CHARS - 1);
        assert_eq!(fitted(&content, &(0..0), "e\u{301}x"), "");
        assert_eq!(fitted(&content[1..], &(0..0), "e\u{301}x"), "e\u{301}");
        // What fits is taken as it is.
        assert_eq!(fitted("dunes", &(5..5), " kind:image"), " kind:image");
    }

    /// A key typed or a paste in a full field changes nothing, and is no
    /// step to take back (`replace_text_in_range` returns at once): held,
    /// typing on never pushes the steps before out of the history. What is
    /// selected is still replaced.
    #[test]
    fn an_edit_a_full_field_takes_nothing_of_changes_nothing() {
        let full = "a".repeat(MOST_CHARS);
        let end = full.len();
        let typed = fitted(&full, &(end..end), "x");
        assert!(changes_nothing(&(end..end), typed, false));
        let pasted = fitted(&full, &(0..0), "it's here");
        assert!(changes_nothing(&(0..0), pasted, false));
        // Selected, it is replaced by what fits, here nothing.
        assert!(!changes_nothing(
            &(0..1),
            fitted(&full, &(0..1), "e\u{301}"),
            false
        ));
        // A composition this ends is a change.
        assert!(!changes_nothing(&(end..end), "", true));
    }

    /// A text a command gives a field, or a completion, is held as a paste
    /// in it empty would be: whole graphemes, the bound at most.
    #[test]
    fn a_text_given_whole_is_held_to_the_bound() {
        let given = "é".repeat(MOST_CHARS + 10);
        let taken = held(given.clone());
        assert_eq!(taken.chars().count(), MOST_CHARS);
        assert!(given.starts_with(&taken));
        assert_eq!(held("kind:image".to_owned()), "kind:image");
    }

    /// What is selected is written in the selection's text color, on the
    /// selection alone, cut out of the runs it falls in; the rest is as it
    /// was (a term's tint, its color).
    #[test]
    fn the_selected_text_is_written_in_the_selection_s_color() {
        let run = |len, color: gpui::Hsla| gpui::TextRun {
            len,
            font: gpui::font("Inter"),
            color,
            background_color: Some(gpui::red()),
            underline: None,
            strikethrough: None,
            position: gpui::RunPosition::Baseline,
        };
        let (a, b, on) = (gpui::black(), gpui::blue(), gpui::white());
        let runs = selected_runs(vec![run(5, a), run(5, b)], &(3..7), on);
        let parts: Vec<_> = runs
            .iter()
            .map(|r| (r.len, r.color, r.background_color.is_some()))
            .collect();
        assert_eq!(
            parts,
            [(3, a, true), (2, on, false), (2, on, false), (3, b, true)]
        );
        // Nothing selected: the runs as they were.
        assert_eq!(selected_runs(vec![run(5, a)], &(2..2), on).len(), 1);
    }

    #[test]
    fn styles_cut_the_run_where_they_fall_and_leave_out_what_does_not_fit() {
        let run = gpui::TextRun {
            len: 0,
            font: gpui::font("Inter"),
            color: gpui::black(),
            background_color: None,
            underline: None,
            strikethrough: None,
            position: gpui::RunPosition::Baseline,
        };
        let key = super::Paint {
            color: Some(gpui::red()),
            ..super::Paint::default()
        };
        let wrong = super::Paint {
            wavy: Some(gpui::red()),
            ..super::Paint::default()
        };
        // `dunes kind:pix`: the key, the error; one style past the text,
        // one inside a character, one overlapping.
        let text = "dunes kind:pix é";
        let styles = vec![
            (6..11, key),
            (6..14, wrong),
            (11..14, wrong),
            (15..16, key),
            (40..41, key),
        ];
        let runs = super::styled_runs(&run, text, &styles);
        let lens: Vec<usize> = runs.iter().map(|r| r.len).collect();
        assert_eq!(lens, [6, 5, 3, 3]);
        assert_eq!(lens.iter().sum::<usize>(), text.len());
        assert_eq!(runs[1].color, gpui::red());
        assert!(runs[2].underline.is_some_and(|u| u.wavy));
        assert!(runs[3].underline.is_none());
    }

    #[test]
    fn an_edit_keeps_the_styles_wholly_before_it() {
        let key = super::Paint {
            color: Some(gpui::red()),
            ..super::Paint::default()
        };
        // `dunes kind:im`: the key, then the value being typed.
        let mut styles = vec![(6..11, key), (11..13, key)];
        // `a` typed at the end: both still cover what they did.
        super::kept_before(&mut styles, 13);
        assert_eq!(styles.len(), 2);
        // A character deleted inside the value: its style waits for the
        // next reading.
        super::kept_before(&mut styles, 12);
        assert_eq!(styles, [(6..11, key)]);
    }

    #[test]
    fn a_screen_reader_counts_the_units_the_caret_moves_by() {
        // `é` is two bytes; a thumb with its skin tone is one grapheme of
        // eight.
        let lengths = units("aé👍🏽");
        assert_eq!(lengths, [1, 2, 8]);
        assert_eq!(unit_at(&lengths, 0), 0);
        assert_eq!(unit_at(&lengths, 1), 1);
        assert_eq!(unit_at(&lengths, 3), 2);
        assert_eq!(unit_at(&lengths, 11), 3);
        // A grapheme longer than a byte counts is split into its chars:
        // `e` and 130 combining accents, 261 bytes.
        let long = format!("e{}", "\u{301}".repeat(130));
        let lengths = units(&long);
        assert_eq!(lengths.len(), 131);
        assert_eq!(
            lengths.iter().map(|&len| usize::from(len)).sum::<usize>(),
            long.len()
        );
    }

    #[test]
    fn an_offset_given_from_outside_never_splits_a_character() {
        // `café.txt`: an agent's `select: [0, 4]` ends inside `é`.
        assert_eq!(snapped("café.txt", 4), 3);
        assert_eq!(snapped("café.txt", 5), 5);
        assert_eq!(snapped("café.txt", 99), 9);
    }
    use gpui::px;

    #[test]
    fn typing_is_one_step_and_other_edits_are_one_each() {
        let mut history = History::default();
        let snap = |text: &str| (text.to_owned().into(), text.len()..text.len());
        // "ab" typed, then a space, then "c", then a deletion.
        history.record(snap(""), typed_at(&(0..0), "a"));
        history.record(snap("a"), typed_at(&(1..1), "b"));
        history.record(snap("ab"), typed_at(&(2..2), " "));
        history.record(snap("ab "), typed_at(&(3..3), "c"));
        history.record(snap("ab c"), typed_at(&(3..4), ""));
        let undo = |history: &mut History, now: &str| {
            history
                .undo(snap(now))
                .map(|((text, _), _)| text.to_string())
        };
        assert_eq!(undo(&mut history, "ab ").as_deref(), Some("ab c"));
        assert_eq!(undo(&mut history, "ab c").as_deref(), Some("ab "));
        assert_eq!(undo(&mut history, "ab ").as_deref(), Some("ab"));
        assert_eq!(undo(&mut history, "ab").as_deref(), Some(""));
        assert_eq!(undo(&mut history, ""), None);
        assert_eq!(
            history
                .redo(snap(""))
                .map(|((text, _), _)| text.to_string())
                .as_deref(),
            Some("ab")
        );
        // A new edit forgets what could be redone.
        history.record(snap("ab"), typed_at(&(2..2), "x"));
        assert!(history.redo(snap("abx")).is_none());
    }

    /// An edit of several fields at once (a recipe's pattern and its
    /// replacement) is a step of each field's history, known by its id:
    /// taken back in one, it is taken back in the other where it is still
    /// the last edit, and put back alike; an edit typed after it in a field
    /// is that field's own, never taken back for the other.
    #[test]
    fn an_edit_of_several_fields_is_one_step_of_each() {
        let snap = |text: &str| -> Snapshot { (text.to_owned().into(), text.len()..text.len()) };
        let text = |step: Option<Snapshot>| step.map(|(text, _)| text.to_string());
        let together = |id| Together {
            id,
            with: Vec::new(),
        };
        let (mut regex, mut replacement) = (History::default(), History::default());
        regex.record(snap(""), typed_at(&(0..0), "x"));
        regex.record_together(snap("x"), together(7));
        replacement.record_together(snap("old"), together(7));
        // Ctrl+Z in the regex: its step says the edit, which the
        // replacement takes back too.
        let (back, said) = regex.undo(snap(r"\s+")).unwrap();
        assert_eq!(back.0.as_ref(), "x");
        assert_eq!(said.map(|t| t.id), Some(7));
        assert_eq!(
            text(replacement.undo_together(7, snap("_"))).as_deref(),
            Some("old")
        );
        // Redo in the replacement puts both back.
        let (again, said) = replacement.redo(snap("old")).unwrap();
        assert_eq!(again.0.as_ref(), "_");
        assert_eq!(said.map(|t| t.id), Some(7));
        assert_eq!(
            text(regex.redo_together(7, snap("x"))).as_deref(),
            Some(r"\s+")
        );
        // Typed on after it, the replacement's last step is its own: the
        // edit taken back in the regex leaves it.
        replacement.record(snap("_"), typed_at(&(1..1), "y"));
        assert!(regex.undo(snap(r"\s+")).is_some());
        assert!(replacement.undo_together(7, snap("_y")).is_none());
        assert!(regex.redo_together(8, snap("x")).is_none(), "another edit");
        // Typing after the edit starts a step of its own.
        let mut field = History::default();
        field.record_together(snap(""), together(9));
        field.record(snap("ab"), typed_at(&(2..2), "c"));
        assert_eq!(field.undo.len(), 2);
    }

    #[test]
    fn typing_elsewhere_starts_a_step() {
        let mut history = History::default();
        let snap = |text: &str| (text.to_owned().into(), 0..0);
        history.record(snap("xy"), typed_at(&(2..2), "a"));
        // The cursor moved to the start before the next character.
        history.record(snap("xya"), typed_at(&(0..0), "b"));
        assert_eq!(history.undo.len(), 2);
    }

    #[test]
    fn a_long_line_scrolls_just_enough_to_show_the_cursor() {
        // Fits: never scrolled, wherever the cursor is.
        assert_eq!(scrolled(px(40.), px(90.), px(100.), px(90.), false), px(0.));
        // The cursor at the end of a 300 px line in a 100 px field: its bar
        // ends at the field's right edge.
        assert_eq!(
            scrolled(px(0.), px(300.), px(100.), px(300.), false),
            px(202.)
        );
        // Moving within what is shown scrolls nothing.
        assert_eq!(
            scrolled(px(202.), px(300.), px(100.), px(250.), false),
            px(202.)
        );
        // Past the left edge: the cursor becomes the first thing shown.
        assert_eq!(
            scrolled(px(202.), px(300.), px(100.), px(50.), false),
            px(50.)
        );
        // A line that got shorter (a deletion) is not left scrolled past its end.
        assert_eq!(
            scrolled(px(202.), px(150.), px(100.), px(150.), false),
            px(52.)
        );
    }

    /// Laid out right to left, a line that fits stands at the field's
    /// right, the cursor's bar inside it; one that does not scrolls as it
    /// does left to right.
    #[test]
    fn a_field_laid_out_right_to_left_starts_at_its_right() {
        // 90 px in 100: the line's x at the field's left edge is -8, so its
        // right end and the cursor's 2 px bar meet the right edge.
        assert_eq!(scrolled(px(0.), px(90.), px(100.), px(0.), true), px(-8.));
        assert_eq!(scrolled(px(40.), px(90.), px(100.), px(90.), true), px(-8.));
        // Longer than the field: the same scrolling as left to right.
        assert_eq!(
            scrolled(px(202.), px(300.), px(100.), px(250.), true),
            px(202.)
        );
        assert_eq!(
            scrolled(px(202.), px(300.), px(100.), px(50.), true),
            px(50.)
        );
        // Unfocused, from its start: a Hebrew line's first character is at
        // its right end, shown as it is; a Latin one's at its left.
        assert_eq!(
            scrolled(px(202.), px(300.), px(100.), px(298.), true),
            px(202.)
        );
        assert_eq!(scrolled(px(202.), px(300.), px(100.), px(0.), true), px(0.));
    }

    #[test]
    fn a_drag_held_past_an_end_selects_on_faster_the_further_it_is() {
        // A field from 100 to 300 px, its line scrolled by 50.
        let at = |pointer: f32| autoscroll_x(px(pointer), px(100.), px(300.), px(50.));
        assert_eq!(at(200.), None, "over the field: nothing goes on");
        // Past the right end: past what is shown (50 + 200), by as far as
        // the pointer is, within 2 and 40 px.
        assert_eq!(at(301.), Some(px(252.)));
        assert_eq!(at(310.), Some(px(260.)));
        assert_eq!(at(900.), Some(px(290.)));
        // Past the left: back from what is shown, never before the start.
        assert_eq!(at(90.), Some(px(40.)));
        assert_eq!(at(0.), Some(px(10.)));
        assert_eq!(
            autoscroll_x(px(0.), px(100.), px(300.), px(5.)),
            Some(px(0.))
        );
    }

    #[test]
    fn utf16_offsets_become_byte_offsets_within_the_composed_text() {
        assert_eq!(utf16_to_utf8("abc", 2), 2);
        assert_eq!(utf16_to_utf8("été", 1), 2, "é is two bytes, one unit");
        assert_eq!(utf16_to_utf8("a😀b", 3), 5, "an emoji is two units");
        assert_eq!(utf16_to_utf8("ab", 9), 2, "clamped to the end");
    }
}
