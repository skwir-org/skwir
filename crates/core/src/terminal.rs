//! A terminal as core-owned data: the screen the UI draws, and what the UI
//! sends it (ADR-019).
//!
//! The terminal plugin runs a shell on a pseudo-terminal and keeps its
//! screen with a VT engine; the UI draws a [`Screen`] and turns keys, typed
//! text, the wheel and the pointer into [`Input`]. Neither knows the other:
//! the engine is the plugin's, gpui is the UI's, and swapping either leaves
//! this file alone. A session publishes a new screen, advances its revision
//! and wakes the app; the UI reads the latest one when it next draws. The UI
//! never waits on a session: [`TerminalSession::send`] only enqueues.

use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use parking_lot::RwLock;

use crate::location::Waker;

/// A colour as the terminal's programs asked for it. The sixteen ANSI
/// colours stay symbolic, so the UI draws them from its theme; the 256-colour
/// cube and true colour arrive as RGB.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Color {
    /// The theme's text colour.
    Foreground,
    /// The theme's background.
    Background,
    /// One of the sixteen ANSI colours, 0..=15.
    Ansi(u8),
    Rgb(u8, u8, u8),
}

/// How a run of text is drawn, beside its colours.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Attrs(u8);

impl Attrs {
    pub const BOLD: Attrs = Attrs(1);
    pub const ITALIC: Attrs = Attrs(1 << 1);
    pub const UNDERLINE: Attrs = Attrs(1 << 2);
    pub const STRIKE: Attrs = Attrs(1 << 3);
    /// Faint text (SGR 2).
    pub const DIM: Attrs = Attrs(1 << 4);

    #[must_use]
    pub fn with(self, other: Attrs, on: bool) -> Attrs {
        if on { Attrs(self.0 | other.0) } else { self }
    }

    pub fn has(self, other: Attrs) -> bool {
        self.0 & other.0 == other.0
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Style {
    pub fg: Color,
    pub bg: Color,
    pub attrs: Attrs,
}

impl Default for Style {
    fn default() -> Self {
        Self {
            fg: Color::Foreground,
            bg: Color::Background,
            attrs: Attrs::default(),
        }
    }
}

/// Text in one style from column `col`, `cells` columns wide. A run is
/// drawn at its column, never after the run before it, so a glyph wider
/// or narrower than a cell cannot push the rest of the line off the grid.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Run {
    pub col: u16,
    pub cells: u16,
    pub text: String,
    pub style: Style,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CursorShape {
    Block,
    Beam,
    Underline,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Cursor {
    pub row: u16,
    pub col: u16,
    pub shape: CursorShape,
    /// It stands on a double-width character.
    pub wide: bool,
}

/// Selected cells on screen, from `start` to `end` inclusive as
/// `(row, col)`. A block selection is the rectangle between them; otherwise
/// it runs along the lines, whole lines between the first and the last.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Selection {
    pub start: (u16, u16),
    pub end: (u16, u16),
    pub block: bool,
}

/// Where a link of the terminal leads (ADR-029).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Target {
    /// An address: one a program wrote as a link, or a web or mail address
    /// found in its text. What may open it is the UI's to decide.
    Uri(String),
    /// A file or a folder that is there, absolute, and the line and column
    /// a message points at in it (`src/main.rs:12:5`).
    Path {
        path: PathBuf,
        line: Option<u32>,
        column: Option<u32>,
    },
}

/// A link on screen: what a program wrote as one (OSC 8), or an address or
/// a path the terminal found in its text. A long one wraps over rows.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Link {
    pub target: Target,
    /// The cells it covers, top to bottom: row, first and last column.
    pub spans: Vec<(u16, u16, u16)>,
    /// A program wrote it as a link: its text may say something else than
    /// where it leads, which is said before it is followed.
    pub written: bool,
}

impl Link {
    /// Whether the link covers the cell at `row`, `col`.
    pub fn covers(&self, row: u16, col: u16) -> bool {
        self.spans
            .iter()
            .any(|&(r, first, last)| r == row && (first..=last).contains(&col))
    }
}

/// What the terminal shows, whole: the UI draws it as it is.
#[derive(Clone, Debug, Default)]
pub struct Screen {
    pub cols: u16,
    pub rows: u16,
    /// One entry per row on screen; blank cells in the background colour
    /// are left out.
    pub lines: Vec<Vec<Run>>,
    /// `None` while a program hides it or the view is scrolled away.
    pub cursor: Option<Cursor>,
    pub selection: Option<Selection>,
    /// The selection's text, as it would be copied.
    pub selected_text: Option<String>,
    /// The links on screen, top to bottom.
    pub links: Vec<Link>,
    /// Lines scrolled back into the history, 0 at the bottom.
    pub scrolled: usize,
    /// Lines of history above the screen.
    pub history: usize,
    /// What the running program called itself (OSC 0/2).
    pub title: String,
    /// The shell's working directory, when the shell has the terminal.
    pub cwd: Option<PathBuf>,
    /// Advances each time the user changed directory in the terminal (not
    /// when it followed the pane: see [`Input::Follow`]), so a pane
    /// following the terminal acts once per change.
    pub moved: u64,
    /// The shell has ended, or the session was stopped.
    pub exited: bool,
}

/// A key the terminal encodes for the program, as the modes it is in
/// require (cursor keys, keypad, modifiers).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum KeyCode {
    /// A character with Ctrl or Alt; plain text is [`Input::Text`].
    Char(char),
    Enter,
    Tab,
    Backspace,
    Escape,
    Up,
    Down,
    Left,
    Right,
    Home,
    End,
    PageUp,
    PageDown,
    Insert,
    Delete,
    /// F1..=F12.
    F(u8),
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
#[allow(clippy::struct_excessive_bools)] // Three independent modifier keys.
pub struct Mods {
    pub ctrl: bool,
    pub alt: bool,
    pub shift: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Key {
    pub code: KeyCode,
    pub mods: Mods,
}

/// The cells the terminal lays out and the pixels each takes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Size {
    pub cols: u16,
    pub rows: u16,
    pub cell_width: u16,
    pub cell_height: u16,
}

pub type Rgb = [u8; 3];

/// The colours the UI draws with, so the terminal can answer a program
/// that asks for them (OSC 4, 10, 11: an editor choosing light or dark).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Palette {
    pub ansi: [Rgb; 16],
    pub fg: Rgb,
    pub bg: Rgb,
}

/// How much a selection takes per step of the pointer.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Unit {
    Char,
    /// A double click.
    Word,
    /// A triple click.
    Line,
}

/// A point on screen for the pointer: a cell, and whether the pointer is in
/// its right half (a selection starting there begins at the next cell).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct At {
    pub row: u16,
    pub col: u16,
    pub right: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Input {
    /// Text typed, composed or committed by an input method.
    Text(String),
    Key(Key),
    /// Text from the clipboard: bracketed when the program asked for it.
    Paste(String),
    Resize(Size),
    /// The wheel turned by `lines`, positive towards older lines. A program
    /// on the alternate screen gets arrow keys; otherwise the history
    /// scrolls.
    Wheel(i32),
    /// Pages of history, positive towards older lines; 0 returns to the
    /// bottom.
    Page(i32),
    Select {
        at: At,
        unit: Unit,
    },
    Extend(At),
    Deselect,
    /// The pane shows this folder: change to it, if the shell has the
    /// terminal and is not there already. Ignored when following is off.
    Follow(PathBuf),
    Palette(Palette),
    /// Whether the panel shows the session. Hidden, it publishes a screen
    /// only for what the window must act on (the shell ending, or going
    /// to another folder the pane follows), never for each output: each
    /// screen published wakes the window, which draws.
    Shown(bool),
}

pub trait TerminalSession: Send + Sync {
    /// Enqueue; never waits for the shell or the pseudo-terminal.
    fn send(&self, input: Input);
    /// The latest screen.
    fn screen(&self) -> Arc<Screen>;
    /// Advances with each screen published.
    fn revision(&self) -> u64;
}

/// What a new terminal starts with.
#[derive(Clone, Debug)]
pub struct Spawn {
    pub dir: PathBuf,
    pub size: Size,
    pub palette: Palette,
}

pub trait TerminalProvider: Send + Sync {
    /// Start a shell in `spawn.dir`. It starts a process: call it from a
    /// worker. `waker` is called after each screen is published.
    fn open(&self, spawn: Spawn, waker: Waker) -> anyhow::Result<Arc<dyn TerminalSession>>;
}

/// Who runs terminals, if anyone: one provider, tagged with its plugin.
#[derive(Default)]
pub struct Terminals {
    provider: RwLock<Option<(String, Arc<dyn TerminalProvider>)>>,
    revision: AtomicU64,
}

impl Terminals {
    pub fn register(&self, owner: &str, provider: Arc<dyn TerminalProvider>) {
        *self.provider.write() = Some((owner.to_owned(), provider));
        self.revision.fetch_add(1, Ordering::Release);
    }

    pub fn provider(&self) -> Option<Arc<dyn TerminalProvider>> {
        self.provider.read().as_ref().map(|(_, p)| Arc::clone(p))
    }

    /// The plugin that runs terminals, for whoever checks it left no trace.
    pub fn owner(&self) -> Option<String> {
        self.provider
            .read()
            .as_ref()
            .map(|(owner, _)| owner.clone())
    }

    /// Advances when a provider comes or goes.
    pub fn revision(&self) -> u64 {
        self.revision.load(Ordering::Acquire)
    }

    pub fn unregister_owner(&self, owner: &str) {
        let mut provider = self.provider.write();
        if provider.as_ref().is_some_and(|(o, _)| o == owner) {
            *provider = None;
            self.revision.fetch_add(1, Ordering::Release);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Nothing;
    impl TerminalProvider for Nothing {
        fn open(&self, _: Spawn, _: Waker) -> anyhow::Result<Arc<dyn TerminalSession>> {
            anyhow::bail!("no terminal here")
        }
    }

    #[test]
    fn a_provider_is_withdrawn_with_its_owner_only() {
        let terminals = Terminals::default();
        assert!(terminals.provider().is_none());
        terminals.register("terminal", Arc::new(Nothing));
        let registered = terminals.revision();
        terminals.unregister_owner("someone-else");
        assert_eq!(terminals.owner().as_deref(), Some("terminal"));
        assert_eq!(terminals.revision(), registered);
        terminals.unregister_owner("terminal");
        assert!(terminals.provider().is_none() && terminals.owner().is_none());
        assert!(terminals.revision() > registered);
    }

    #[test]
    fn attributes_combine() {
        let attrs = Attrs::default()
            .with(Attrs::BOLD, true)
            .with(Attrs::ITALIC, false)
            .with(Attrs::DIM, true);
        assert!(attrs.has(Attrs::BOLD) && attrs.has(Attrs::DIM));
        assert!(!attrs.has(Attrs::ITALIC) && !attrs.has(Attrs::UNDERLINE));
    }
}
