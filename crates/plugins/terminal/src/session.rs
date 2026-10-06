//! One terminal: a shell on a pseudo-terminal, the engine that keeps its
//! screen, and the thread that answers the UI.
//!
//! Three threads, none of them the UI's. The engine's event loop reads the
//! shell's output and parses it into the grid. This session's thread takes
//! what the UI sent (keys, text, the wheel, the pointer, a new size), and
//! after output publishes a new [`Screen`] and wakes the app; at most one
//! screen per [`INTERVAL`], so a flood of output costs the UI a frame per
//! display refresh, not one per read. Idle, both threads sleep in the
//! kernel: nothing polls.

use std::borrow::Cow;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, OnceLock};
use std::time::{Duration, Instant};

use alacritty_terminal::event::{Event, EventListener, WindowSize};
use alacritty_terminal::event_loop::{EventLoop, EventLoopSender, Msg};
use alacritty_terminal::grid::{Dimensions, Scroll};
use alacritty_terminal::index::{Column, Line, Point, Side};
use alacritty_terminal::selection::{Selection, SelectionType};
use alacritty_terminal::sync::FairMutex;
use alacritty_terminal::term::test::TermSize;
use alacritty_terminal::term::{Config, Osc52, Term, TermMode};
use alacritty_terminal::tty;
use bibli_core::location::Waker;
use bibli_core::terminal::{
    At, Input, Key, KeyCode, Mods, Palette, Screen, Size, Spawn, TerminalSession, Unit,
};
use parking_lot::{Condvar, Mutex, RwLock};

use crate::follow::{self, Follow};
use crate::{keys, links, screen};

/// What the session keeps to ask who has the terminal: the controller's
/// descriptor on Unix; nothing yet on Windows (ADR-035).
#[cfg(unix)]
type Controller = std::os::fd::OwnedFd;
#[cfg(not(unix))]
type Controller = ();

/// The least time between two screens published: about one frame.
const INTERVAL: Duration = Duration::from_millis(8);

/// How a terminal is started, from the plugin's settings.
#[derive(Clone, Debug)]
pub struct Options {
    /// The shell; `None` for the user's login shell.
    pub program: Option<String>,
    /// Lines of history kept above the screen.
    pub scrollback: usize,
    /// Whether the terminal and the pane keep to the same folder; shared by
    /// every session, so a change in Settings reaches the open ones.
    pub follow: Arc<AtomicBool>,
}

#[derive(Default)]
struct State {
    inputs: Vec<Input>,
    /// The engine has news: output was parsed, the title changed.
    dirty: bool,
    exited: bool,
    stopped: bool,
}

/// What the session's threads share with the engine's listener and with
/// the UI's handle.
pub(crate) struct Shared {
    state: Mutex<State>,
    nudge: Condvar,
    screen: RwLock<Arc<Screen>>,
    revision: AtomicU64,
    waker: Waker,
    /// The event loop's inbox: bytes for the shell, a new size, shutdown.
    sender: OnceLock<EventLoopSender>,
    palette: Mutex<Palette>,
    size: Mutex<WindowSize>,
    title: Mutex<String>,
}

impl Shared {
    fn with_state(&self, change: impl FnOnce(&mut State)) {
        change(&mut self.state.lock());
        self.nudge.notify_one();
    }

    fn write(&self, bytes: impl Into<Cow<'static, [u8]>>) {
        if let Some(sender) = self.sender.get() {
            let _ = sender.send(Msg::Input(bytes.into()));
        }
    }

    /// End the session: its thread stops the event loop, which hangs up
    /// on the shell.
    pub(crate) fn stop(&self) {
        self.with_state(|state| state.stopped = true);
    }
}

/// The engine's listener. It is called on the event loop's thread, often
/// with the grid locked: it must never lock the grid itself.
struct Listener(Arc<Shared>);

impl EventListener for Listener {
    fn send_event(&self, event: Event) {
        let shared = &self.0;
        match event {
            Event::Wakeup | Event::MouseCursorDirty => shared.with_state(|s| s.dirty = true),
            Event::Title(title) => {
                *shared.title.lock() = title;
                shared.with_state(|s| s.dirty = true);
            }
            Event::ResetTitle => {
                shared.title.lock().clear();
                shared.with_state(|s| s.dirty = true);
            }
            // Answers the terminal owes the program: device attributes,
            // cursor position, the colours and size it asks about.
            Event::PtyWrite(text) => shared.write(text.into_bytes()),
            Event::ColorRequest(index, format) => {
                let [r, g, b] = screen::answer(&shared.palette.lock(), index);
                shared.write(format(alacritty_terminal::vte::ansi::Rgb { r, g, b }).into_bytes());
            }
            Event::TextAreaSizeRequest(format) => {
                let size = *shared.size.lock();
                shared.write(format(size).into_bytes());
            }
            Event::ChildExit(_) | Event::Exit => shared.with_state(|s| {
                s.exited = true;
                s.dirty = true;
            }),
            // The clipboard is not a program's to read or write (OSC 52 is
            // off), and nothing blinks or rings.
            Event::ClipboardStore(..)
            | Event::ClipboardLoad(..)
            | Event::CursorBlinkingChange
            | Event::Bell => {}
        }
    }
}

/// The UI's handle. Dropping it ends the session.
pub struct Session {
    shared: Arc<Shared>,
}

impl TerminalSession for Session {
    fn send(&self, input: Input) {
        self.shared.with_state(|state| state.inputs.push(input));
    }

    fn screen(&self) -> Arc<Screen> {
        Arc::clone(&self.shared.screen.read())
    }

    fn revision(&self) -> u64 {
        self.shared.revision.load(Ordering::Acquire)
    }
}

impl Drop for Session {
    fn drop(&mut self) {
        self.shared.stop();
    }
}

fn window_size(size: Size) -> WindowSize {
    WindowSize {
        num_lines: size.rows.max(1),
        num_cols: size.cols.max(2),
        cell_width: size.cell_width.max(1),
        cell_height: size.cell_height.max(1),
    }
}

fn term_size(size: WindowSize) -> TermSize {
    TermSize::new(usize::from(size.num_cols), usize::from(size.num_lines))
}

/// Start a shell in `spawn.dir`. Returns the UI's handle and what the
/// plugin keeps to stop it.
pub(crate) fn open(
    spawn: Spawn,
    waker: Waker,
    options: &Options,
) -> anyhow::Result<(Arc<Session>, Arc<Shared>)> {
    let size = window_size(spawn.size);
    let shared = Arc::new(Shared {
        state: Mutex::default(),
        nudge: Condvar::new(),
        screen: RwLock::default(),
        revision: AtomicU64::new(0),
        waker,
        sender: OnceLock::new(),
        palette: Mutex::new(spawn.palette),
        size: Mutex::new(size),
        title: Mutex::default(),
    });
    let config = Config {
        scrolling_history: options.scrollback,
        osc52: Osc52::Disabled,
        ..Config::default()
    };
    let term = Arc::new(FairMutex::new(Term::new(
        config,
        &term_size(size),
        Listener(Arc::clone(&shared)),
    )));
    let env = HashMap::from([
        ("TERM".to_owned(), "xterm-256color".to_owned()),
        ("COLORTERM".to_owned(), "truecolor".to_owned()),
        ("TERM_PROGRAM".to_owned(), "bibli".to_owned()),
    ]);
    let pty = tty::new(
        &tty::Options {
            shell: options
                .program
                .clone()
                .map(|program| tty::Shell::new(program, Vec::new())),
            working_directory: Some(spawn.dir.clone()),
            drain_on_exit: true,
            env,
            #[cfg(windows)]
            escape_args: true,
        },
        size,
        0,
    )?;
    // Kept to ask who has the terminal; the event loop owns the original.
    #[cfg(unix)]
    let (controller, pid): (Controller, u32) = (pty.file().try_clone()?.into(), pty.child().id());
    #[cfg(windows)]
    let (controller, pid): (Controller, u32) = (
        (),
        pty.child_watcher()
            .pid()
            .map_or(0, std::num::NonZeroU32::get),
    );
    let family = options
        .program
        .clone()
        .or_else(|| std::env::var("SHELL").ok())
        .and_then(|program| follow::family(&program));
    let event_loop = EventLoop::new(
        Arc::clone(&term),
        Listener(Arc::clone(&shared)),
        pty,
        true,
        false,
    )?;
    let _ = shared.sender.set(event_loop.channel());
    // Detached: when the shell ends or the session stops, the loop returns
    // and the pty is dropped on its own thread (hang up, then reap).
    drop(event_loop.spawn());
    let found = Arc::clone(&shared);
    let worker = Worker {
        shared: Arc::clone(&shared),
        term,
        controller,
        pid,
        family,
        follow_on: Arc::clone(&options.follow),
        cwd: Some(spawn.dir.clone()),
        follow: Follow::starting_in(spawn.dir),
        // A path found to lead somewhere is a link more to publish.
        links: links::Finder::new(move || found.with_state(|state| state.dirty = true)),
    };
    std::thread::Builder::new()
        .name("bibli-terminal".into())
        .spawn(move || worker.run())?;
    Ok((
        Arc::new(Session {
            shared: Arc::clone(&shared),
        }),
        shared,
    ))
}

/// The session's thread.
struct Worker {
    shared: Arc<Shared>,
    term: Arc<FairMutex<Term<Listener>>>,
    controller: Controller,
    pid: u32,
    family: Option<follow::Family>,
    follow_on: Arc<AtomicBool>,
    follow: Follow,
    /// Where the shell is, as last read: what paths printed now are read
    /// from.
    cwd: Option<PathBuf>,
    links: links::Finder,
}

impl Worker {
    fn run(mut self) {
        let mut published = Instant::now()
            .checked_sub(INTERVAL)
            .unwrap_or_else(Instant::now);
        let mut pending = true;
        loop {
            let (inputs, exited) = {
                let mut state = self.shared.state.lock();
                loop {
                    pending |= std::mem::take(&mut state.dirty);
                    if state.stopped || !state.inputs.is_empty() {
                        break;
                    }
                    if !pending {
                        self.shared.nudge.wait(&mut state);
                        continue;
                    }
                    let since = published.elapsed();
                    if since >= INTERVAL {
                        break;
                    }
                    self.shared
                        .nudge
                        .wait_for(&mut state, INTERVAL.saturating_sub(since));
                }
                if state.stopped {
                    break;
                }
                (std::mem::take(&mut state.inputs), state.exited)
            };
            for input in inputs {
                pending |= self.apply(input);
            }
            if pending && published.elapsed() >= INTERVAL {
                self.publish(exited);
                published = Instant::now();
                pending = false;
                if exited {
                    break;
                }
            }
        }
        if let Some(sender) = self.shared.sender.get() {
            let _ = sender.send(Msg::Shutdown);
        }
    }

    /// Whether the shell's process group has the terminal: a program it
    /// runs in the foreground has it instead.
    #[cfg(unix)]
    fn shell_has_terminal(&self) -> bool {
        rustix::termios::tcgetpgrp(&self.controller)
            .is_ok_and(|group| u32::try_from(group.as_raw_nonzero().get()).ok() == Some(self.pid))
    }

    /// Not known off Unix yet (ADR-035): never, so no `cd` is typed over
    /// a program the shell runs.
    #[cfg(not(unix))]
    #[allow(clippy::unused_self)]
    fn shell_has_terminal(&self) -> bool {
        let () = self.controller;
        false
    }

    /// Where the shell is now, as the system says
    /// (`process::working_dir`: not on Windows yet, ADR-035).
    fn shell_dir(&self) -> Option<PathBuf> {
        bibli_platform::process::working_dir(self.pid)
    }

    fn type_cd(&self, dir: &std::path::Path) {
        if let Some(line) = self.family.and_then(|family| follow::cd_line(family, dir)) {
            self.shared.write(line);
        }
    }

    fn mode(&self) -> TermMode {
        *self.term.lock().mode()
    }

    /// Typing returns the view to the bottom, where the prompt is.
    fn to_bottom(&self) -> bool {
        let mut term = self.term.lock();
        let scrolled = term.grid().display_offset() > 0;
        if scrolled {
            term.scroll_display(Scroll::Bottom);
        }
        scrolled
    }

    fn point(term: &Term<Listener>, at: At) -> (Point, Side) {
        let offset = i32::try_from(term.grid().display_offset()).unwrap_or(i32::MAX);
        let last = i32::try_from(term.screen_lines()).unwrap_or(i32::MAX) - 1;
        let row = i32::from(at.row).min(last);
        let col = usize::from(at.col).min(term.columns().saturating_sub(1));
        (
            Point::new(Line(row - offset), Column(col)),
            if at.right { Side::Right } else { Side::Left },
        )
    }

    /// Carry out what the UI sent. Returns whether the screen changed.
    fn apply(&mut self, input: Input) -> bool {
        match input {
            Input::Text(text) => {
                self.shared.write(text.into_bytes());
                self.to_bottom()
            }
            Input::Key(key) => {
                let app_cursor = self.mode().contains(TermMode::APP_CURSOR);
                self.shared.write(keys::encode(key, app_cursor));
                self.to_bottom()
            }
            Input::Paste(text) => {
                let bracketed = self.mode().contains(TermMode::BRACKETED_PASTE);
                self.shared.write(keys::paste(&text, bracketed));
                self.to_bottom()
            }
            Input::Resize(size) => {
                let size = window_size(size);
                let mut current = self.shared.size.lock();
                if (
                    current.num_cols,
                    current.num_lines,
                    current.cell_width,
                    current.cell_height,
                ) == (
                    size.num_cols,
                    size.num_lines,
                    size.cell_width,
                    size.cell_height,
                ) {
                    return false;
                }
                *current = size;
                drop(current);
                self.term.lock().resize(term_size(size));
                if let Some(sender) = self.shared.sender.get() {
                    let _ = sender.send(Msg::Resize(size));
                }
                true
            }
            Input::Wheel(lines) => {
                let mode = self.mode();
                if mode.contains(TermMode::ALT_SCREEN | TermMode::ALTERNATE_SCROLL) {
                    // A pager or an editor scrolls itself: give it arrows.
                    let code = if lines > 0 {
                        KeyCode::Up
                    } else {
                        KeyCode::Down
                    };
                    let arrow = keys::encode(
                        Key {
                            code,
                            mods: Mods::default(),
                        },
                        mode.contains(TermMode::APP_CURSOR),
                    );
                    let bytes = arrow.repeat(lines.unsigned_abs() as usize);
                    self.shared.write(bytes);
                    false
                } else {
                    self.term.lock().scroll_display(Scroll::Delta(lines));
                    true
                }
            }
            Input::Page(pages) => {
                let mut term = self.term.lock();
                if pages == 0 {
                    term.scroll_display(Scroll::Bottom);
                }
                for _ in 0..pages.unsigned_abs() {
                    term.scroll_display(if pages > 0 {
                        Scroll::PageUp
                    } else {
                        Scroll::PageDown
                    });
                }
                true
            }
            Input::Select { at, unit } => {
                let mut term = self.term.lock();
                let (point, side) = Self::point(&term, at);
                let kind = match unit {
                    Unit::Char => SelectionType::Simple,
                    Unit::Word => SelectionType::Semantic,
                    Unit::Line => SelectionType::Lines,
                };
                term.selection = Some(Selection::new(kind, point, side));
                true
            }
            Input::Extend(at) => {
                let mut term = self.term.lock();
                let (point, side) = Self::point(&term, at);
                match term.selection.as_mut() {
                    Some(selection) => {
                        selection.update(point, side);
                        true
                    }
                    None => false,
                }
            }
            Input::Deselect => self.term.lock().selection.take().is_some(),
            Input::Follow(dir) => {
                if self.follow_on.load(Ordering::Relaxed) && self.family.is_some() {
                    let foreground = self.shell_has_terminal();
                    if let Some(dir) = self.follow.pane_at(dir, foreground) {
                        self.type_cd(&dir);
                    }
                }
                false
            }
            Input::Palette(palette) => {
                *self.shared.palette.lock() = palette;
                false
            }
        }
    }

    /// Where the shell is, when it is looked at after output: where the
    /// paths it printed are read from, and where the pane follows it.
    fn look_at_shell(&mut self) {
        let Some(cwd) = self.shell_dir() else {
            return;
        };
        self.cwd = Some(cwd.clone());
        if !self.follow_on.load(Ordering::Relaxed) {
            return;
        }
        let foreground = self.shell_has_terminal();
        if let Some(dir) = self.follow.shell_at(&cwd, foreground) {
            self.type_cd(&dir);
        }
    }

    fn publish(&mut self, exited: bool) {
        if !exited {
            self.look_at_shell();
        }
        let mut term = self.term.lock();
        self.links.stamp(&mut term, self.cwd.as_deref());
        let mut screen = screen::build(&term, &self.links.stamps());
        drop(term);
        screen.title.clone_from(&self.shared.title.lock());
        screen.cwd = self.follow.seen().map(PathBuf::from);
        screen.moved = self.follow.moved;
        screen.exited = exited;
        *self.shared.screen.write() = Arc::new(screen);
        self.shared.revision.fetch_add(1, Ordering::Release);
        (self.shared.waker)();
    }
}
