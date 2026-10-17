//! A single-line text input entity: cursor, selection, IME-safe editing,
//! clipboard, word moves. Reused by the address bar, the palette, and prompts.
//! Modeled on gpui's `examples/input.rs`.

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
        KeyBinding::new("secondary-a", SelectAll, c),
        KeyBinding::new("home", Home, c),
        KeyBinding::new("end", End, c),
        KeyBinding::new("shift-home", SelectHome, c),
        KeyBinding::new("shift-end", SelectEnd, c),
        KeyBinding::new("secondary-v", Paste, c),
        KeyBinding::new("secondary-c", Copy, c),
        KeyBinding::new("secondary-x", Cut, c),
        KeyBinding::new("ctrl-backspace", DeleteWordLeft, c),
        KeyBinding::new("ctrl-delete", DeleteWordRight, c),
        // The field's own undo, before the window's (the file operations').
        KeyBinding::new("secondary-z", Undo, c),
        KeyBinding::new("secondary-shift-z", Redo, c),
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

/// The text and selection an edit started from.
type Snapshot = (SharedString, Range<usize>);

/// What Ctrl+Z and Ctrl+Shift+Z walk through. A run of characters typed
/// one after the other is one step, as in any text field; anything else
/// (a deletion, a paste, a composition, typing somewhere else) starts one.
#[derive(Default)]
struct History {
    undo: Vec<Snapshot>,
    redo: Vec<Snapshot>,
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
            if self.undo.len() == Self::DEPTH {
                self.undo.remove(0);
            }
            self.undo.push(before);
        }
        self.redo.clear();
        self.typed_end = typed.map(|t| t.end);
    }

    fn undo(&mut self, current: Snapshot) -> Option<Snapshot> {
        let previous = self.undo.pop()?;
        self.redo.push(current);
        self.typed_end = None;
        Some(previous)
    }

    fn redo(&mut self, current: Snapshot) -> Option<Snapshot> {
        let next = self.redo.pop()?;
        self.undo.push(current);
        self.typed_end = None;
        Some(next)
    }

    fn clear(&mut self) {
        *self = History::default();
    }
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

pub struct TextInput {
    focus_handle: FocusHandle,
    content: SharedString,
    placeholder: SharedString,
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
    /// Kept only for text that is not masked: a passphrase leaves no trail.
    history: History,
}

impl EventEmitter<InputEvent> for TextInput {}

impl TextInput {
    pub fn new(placeholder: impl Into<SharedString>, cx: &mut Context<Self>) -> Self {
        TextInput {
            focus_handle: cx.focus_handle(),
            content: "".into(),
            placeholder: placeholder.into(),
            masked: false,
            selected_range: 0..0,
            selection_reversed: false,
            marked_range: None,
            last_layout: None,
            last_bounds: None,
            scroll_x: px(0.),
            is_selecting: false,
            history: History::default(),
        }
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
    pub fn set_placeholder(
        &mut self,
        placeholder: impl Into<SharedString>,
        cx: &mut Context<Self>,
    ) {
        self.placeholder = placeholder.into();
        cx.notify();
    }

    pub fn text(&self) -> &str {
        &self.content
    }

    /// Replace the whole text and put the cursor at the end.
    pub fn set_text(&mut self, text: impl Into<SharedString>, cx: &mut Context<Self>) {
        self.content = text.into();
        // Text put here is where editing starts, not a step to take back.
        self.history.clear();
        let end = self.content.len();
        self.selected_range = end..end;
        self.selection_reversed = false;
        self.marked_range = None;
        cx.notify();
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
        if self.selected_range.is_empty() {
            self.move_to(self.previous_boundary(self.cursor_offset()), cx);
        } else {
            self.move_to(self.selected_range.start, cx);
        }
    }

    fn right(&mut self, _: &Right, _: &mut Window, cx: &mut Context<Self>) {
        if self.selected_range.is_empty() {
            self.move_to(self.next_boundary(self.selected_range.end), cx);
        } else {
            self.move_to(self.selected_range.end, cx);
        }
    }

    fn select_left(&mut self, _: &SelectLeft, _: &mut Window, cx: &mut Context<Self>) {
        self.select_to(self.previous_boundary(self.cursor_offset()), cx);
    }

    fn select_right(&mut self, _: &SelectRight, _: &mut Window, cx: &mut Context<Self>) {
        self.select_to(self.next_boundary(self.cursor_offset()), cx);
    }

    fn word_left(&mut self, _: &WordLeft, _: &mut Window, cx: &mut Context<Self>) {
        self.move_to(self.previous_word(self.cursor_offset()), cx);
    }

    fn word_right(&mut self, _: &WordRight, _: &mut Window, cx: &mut Context<Self>) {
        self.move_to(self.next_word(self.cursor_offset()), cx);
    }

    fn select_word_left(&mut self, _: &SelectWordLeft, _: &mut Window, cx: &mut Context<Self>) {
        self.select_to(self.previous_word(self.cursor_offset()), cx);
    }

    fn select_word_right(&mut self, _: &SelectWordRight, _: &mut Window, cx: &mut Context<Self>) {
        self.select_to(self.next_word(self.cursor_offset()), cx);
    }

    fn select_all(&mut self, _: &SelectAll, _: &mut Window, cx: &mut Context<Self>) {
        self.move_to(0, cx);
        self.select_to(self.content.len(), cx);
    }

    fn home(&mut self, _: &Home, _: &mut Window, cx: &mut Context<Self>) {
        self.move_to(0, cx);
    }

    fn end(&mut self, _: &End, _: &mut Window, cx: &mut Context<Self>) {
        self.move_to(self.content.len(), cx);
    }

    fn select_home(&mut self, _: &SelectHome, _: &mut Window, cx: &mut Context<Self>) {
        self.select_to(0, cx);
    }

    fn select_end(&mut self, _: &SelectEnd, _: &mut Window, cx: &mut Context<Self>) {
        self.select_to(self.content.len(), cx);
    }

    fn backspace(&mut self, _: &Backspace, window: &mut Window, cx: &mut Context<Self>) {
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
        if let Some(previous) = self.history.undo(current) {
            self.restore(previous, cx);
        }
    }

    fn redo(&mut self, _: &Redo, _: &mut Window, cx: &mut Context<Self>) {
        let current = (self.content.clone(), self.selected_range.clone());
        if let Some(next) = self.history.redo(current) {
            self.restore(next, cx);
        }
    }

    fn restore(&mut self, (content, selected): Snapshot, cx: &mut Context<Self>) {
        self.content = content;
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

    fn on_mouse_down(&mut self, ev: &MouseDownEvent, _: &mut Window, cx: &mut Context<Self>) {
        self.is_selecting = true;
        if ev.click_count >= 2 {
            self.selected_range = 0..self.content.len();
            self.selection_reversed = false;
            cx.notify();
        } else if ev.modifiers.shift {
            self.select_to(self.index_for_mouse_position(ev.position), cx);
        } else {
            self.move_to(self.index_for_mouse_position(ev.position), cx);
        }
    }

    fn on_mouse_up(&mut self, _: &MouseUpEvent, _: &mut Window, _: &mut Context<Self>) {
        self.is_selecting = false;
    }

    fn on_mouse_move(&mut self, ev: &MouseMoveEvent, _: &mut Window, cx: &mut Context<Self>) {
        if self.is_selecting {
            self.select_to(self.index_for_mouse_position(ev.position), cx);
        }
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

    fn index_for_mouse_position(&self, position: Point<Pixels>) -> usize {
        if self.content.is_empty() {
            return 0;
        }
        let (Some(bounds), Some(line)) = (self.last_bounds.as_ref(), self.last_layout.as_ref())
        else {
            return 0;
        };
        if position.y < bounds.top() {
            return 0;
        }
        if position.y > bounds.bottom() {
            return self.content.len();
        }
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

    fn previous_word(&self, offset: usize) -> usize {
        self.content[..offset]
            .unicode_word_indices()
            .next_back()
            .map_or(0, |(i, _)| i)
    }

    fn next_word(&self, offset: usize) -> usize {
        self.content[offset..]
            .unicode_word_indices()
            .next()
            .map_or(self.content.len(), |(i, w)| offset + i + w.len())
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
        // A composition being committed was recorded when it began.
        if self.marked_range.is_none() && !self.masked {
            let before = (self.content.clone(), self.selected_range.clone());
            self.history.record(before, typed_at(&range, new_text));
        }
        self.content =
            (self.content[0..range.start].to_owned() + new_text + &self.content[range.end..])
                .into();
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
        // A composition is one step, from the text before it began.
        if self.marked_range.is_none() && !self.masked {
            let before = (self.content.clone(), self.selected_range.clone());
            self.history.record(before, None);
        }
        self.content =
            (self.content[0..range.start].to_owned() + new_text + &self.content[range.end..])
                .into();
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
        // The layout is of what is shown: bullets, when masked.
        let left = bounds.left() - self.scroll_x;
        Some(Bounds::from_corners(
            point(
                left + last_layout.x_for_index(self.shown_offset(range.start)),
                bounds.top(),
            ),
            point(
                left + last_layout.x_for_index(self.shown_offset(range.end)),
                bounds.bottom(),
            ),
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
    selection: Option<PaintQuad>,
    scroll_x: Pixels,
}

/// How far to scroll a line `width` wide in a field `field` wide, from
/// `scroll`, so the cursor at `cursor` (and its 2 px bar) stays in view:
/// no more than needed, never past either end, and none when it fits.
fn scrolled(scroll: Pixels, width: Pixels, field: Pixels, cursor: Pixels) -> Pixels {
    let bar = px(2.);
    if width + bar <= field {
        return px(0.);
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
            (input.placeholder.clone(), theme.text_muted)
        } else {
            (content, style.color)
        };

        let run = TextRun {
            len: display_text.len(),
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
                    len: display_text.len() - marked.end,
                    ..run
                },
            ]
            .into_iter()
            .filter(|r| r.len > 0)
            .collect()
        } else {
            vec![run]
        };

        let font_size = style.font_size.to_pixels(window.rem_size());
        let line = window
            .text_system()
            .shape_line(display_text, font_size, &runs, None);

        let cursor_pos = line.x_for_index(cursor);
        // Unfocused, a long text shows its start, as a label would.
        let scroll_x = if input.focus_handle.is_focused(window) {
            scrolled(input.scroll_x, line.width, bounds.size.width, cursor_pos)
        } else {
            px(0.)
        };
        let left = bounds.left() - scroll_x;
        let (selection, cursor) = if selected_range.is_empty() {
            (
                None,
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
                Some(fill(
                    Bounds::from_corners(
                        point(left + line.x_for_index(selected_range.start), bounds.top()),
                        point(left + line.x_for_index(selected_range.end), bounds.bottom()),
                    ),
                    theme.selection_bg,
                )),
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
        window.with_content_mask(Some(gpui::ContentMask { bounds }), |window| {
            if let Some(selection) = prepaint.selection.take() {
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
        self.input.update(cx, |input, _| {
            input.last_layout = Some(line);
            input.last_bounds = Some(bounds);
            input.scroll_x = scroll_x;
        });
    }
}

impl Render for TextInput {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        div()
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
            .on_mouse_move(cx.listener(Self::on_mouse_move))
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

#[cfg(test)]
mod tests {
    use super::{History, scrolled, snapped, typed_at, utf16_to_utf8};

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
            history.undo(snap(now)).map(|(text, _)| text.to_string())
        };
        assert_eq!(undo(&mut history, "ab ").as_deref(), Some("ab c"));
        assert_eq!(undo(&mut history, "ab c").as_deref(), Some("ab "));
        assert_eq!(undo(&mut history, "ab ").as_deref(), Some("ab"));
        assert_eq!(undo(&mut history, "ab").as_deref(), Some(""));
        assert_eq!(undo(&mut history, ""), None);
        assert_eq!(
            history
                .redo(snap(""))
                .map(|(text, _)| text.to_string())
                .as_deref(),
            Some("ab")
        );
        // A new edit forgets what could be redone.
        history.record(snap("ab"), typed_at(&(2..2), "x"));
        assert_eq!(history.redo(snap("abx")), None);
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
        assert_eq!(scrolled(px(40.), px(90.), px(100.), px(90.)), px(0.));
        // The cursor at the end of a 300 px line in a 100 px field: its bar
        // ends at the field's right edge.
        assert_eq!(scrolled(px(0.), px(300.), px(100.), px(300.)), px(202.));
        // Moving within what is shown scrolls nothing.
        assert_eq!(scrolled(px(202.), px(300.), px(100.), px(250.)), px(202.));
        // Past the left edge: the cursor becomes the first thing shown.
        assert_eq!(scrolled(px(202.), px(300.), px(100.), px(50.)), px(50.));
        // A line that got shorter (a deletion) is not left scrolled past its end.
        assert_eq!(scrolled(px(202.), px(150.), px(100.), px(150.)), px(52.));
    }

    #[test]
    fn utf16_offsets_become_byte_offsets_within_the_composed_text() {
        assert_eq!(utf16_to_utf8("abc", 2), 2);
        assert_eq!(utf16_to_utf8("été", 1), 2, "é is two bytes, one unit");
        assert_eq!(utf16_to_utf8("a😀b", 3), 5, "an emoji is two units");
        assert_eq!(utf16_to_utf8("ab", 9), 2, "clamped to the end");
    }
}
