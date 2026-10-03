//! The terminal panel (ADR-019): a shell under the panes, as in Dolphin.
//!
//! The terminal plugin runs the shell and keeps its screen; this module
//! opens a session through `App::terminals`, draws the screen it publishes
//! (`elements::terminal`) and sends it what the user does. The panel spans
//! the panes of the tab shown and follows the active one: when that pane
//! goes elsewhere the session is told (`Input::Follow`), and when the user
//! changes directory in the shell (`Screen::moved`) the pane goes there.
//!
//! Hiding the panel (F4) keeps the shell and its history; it ends when the
//! shell exits, when the plugin is disabled, or with the window. The keys
//! the window binds are the shell's while it has the keyboard, but F4 and
//! the palette (`keymap::TERMINAL_PASSTHROUGH`).
//!
//! Its links (ADR-029) follow Ctrl+click, or the keyboard: Ctrl+Shift+O
//! labels them, and typing a label follows its link (in a tab of its own
//! in capitals), as foot's and kitty's hints do.

use std::path::PathBuf;
use std::rc::Rc;
use std::sync::Arc;

use bibli_core::terminal::{
    Input, Key, KeyCode, Mods, Palette, Screen, Size, Spawn, TerminalSession,
};
use gpui::{
    AnyElement, ClipboardItem, Context, CursorStyle, DragMoveEvent, FocusHandle, Hsla,
    KeyDownEvent, Window, div, prelude::*, px,
};

use crate::elements::terminal::{Colors, GridState, TerminalGrid};
use crate::keymap;
use crate::shell::Shell;
use crate::theme::Theme;

/// The panel's height until its edge is dragged, and the least it keeps.
const DEFAULT_HEIGHT: f32 = 240.;
const MIN_HEIGHT: f32 = 90.;
/// The terminal's text size.
const FONT_SIZE: f32 = 13.;

pub struct TerminalState {
    pub open: bool,
    /// As the edge was dragged; `None` until then.
    pub height: Option<f32>,
    pub focus: FocusHandle,
    session: Option<Arc<dyn TerminalSession>>,
    /// A shell is being started on a worker.
    opening: bool,
    screen: Arc<Screen>,
    revision: u64,
    /// `Screen::moved` last acted on.
    moved: u64,
    /// `App::terminals` revision last looked at.
    provider_revision: u64,
    /// The folder the session was last told the pane shows.
    followed: Option<PathBuf>,
    palette: Option<Palette>,
    grid: Rc<GridState>,
    /// The links are labelled (Ctrl+Shift+O): what was typed of a label.
    labelled: Option<String>,
}

impl TerminalState {
    pub fn new(focus: FocusHandle) -> Self {
        Self {
            open: false,
            height: None,
            focus,
            session: None,
            opening: false,
            screen: Arc::default(),
            revision: 0,
            moved: 0,
            provider_revision: 0,
            followed: None,
            palette: None,
            grid: Rc::default(),
            labelled: None,
        }
    }

    /// Label the links, or stop.
    fn label(&mut self, labelled: Option<String>) {
        self.grid.set_labelling(labelled.is_some());
        self.labelled = labelled;
    }

    fn forget_session(&mut self) {
        self.session = None;
        self.screen = Arc::default();
        self.revision = 0;
        self.moved = 0;
        self.followed = None;
        self.palette = None;
        self.grid = Rc::default();
        self.labelled = None;
    }
}

fn rgb(color: Hsla) -> [u8; 3] {
    let rgba = color.to_rgb();
    [rgba.r, rgba.g, rgba.b].map(|channel| (channel.clamp(0., 1.) * 255.).round() as u8)
}

/// The theme's colours, for programs that ask which they are.
fn palette(theme: &Theme) -> Palette {
    Palette {
        ansi: theme.terminal.map(rgb),
        fg: rgb(theme.text),
        bg: rgb(theme.bg),
    }
}

/// A key with no text of its own, or a Ctrl or Alt chord, as the terminal
/// encodes it. Plain text is left to the platform's input handler, which
/// also composes dead keys and takes what an input method commits.
fn key_of(keystroke: &gpui::Keystroke) -> Option<Key> {
    let m = keystroke.modifiers;
    if m.platform || m.function {
        return None;
    }
    let mods = Mods {
        ctrl: m.control,
        alt: m.alt,
        shift: m.shift,
    };
    let code = match keystroke.key.as_str() {
        "enter" => KeyCode::Enter,
        "tab" => KeyCode::Tab,
        "backspace" => KeyCode::Backspace,
        "escape" => KeyCode::Escape,
        "up" => KeyCode::Up,
        "down" => KeyCode::Down,
        "left" => KeyCode::Left,
        "right" => KeyCode::Right,
        "home" => KeyCode::Home,
        "end" => KeyCode::End,
        "pageup" => KeyCode::PageUp,
        "pagedown" => KeyCode::PageDown,
        "insert" => KeyCode::Insert,
        "delete" => KeyCode::Delete,
        key if key.len() > 1 && key.starts_with('f') => {
            KeyCode::F(key[1..].parse().ok().filter(|n| (1..=12).contains(n))?)
        }
        _ if !(m.control || m.alt) => return None,
        "space" => KeyCode::Char(' '),
        key => {
            let text = keystroke
                .key_char
                .as_deref()
                .filter(|text| !text.chars().any(char::is_control))
                .unwrap_or(key);
            let mut chars = text.chars();
            let c = chars.next()?;
            if chars.next().is_some() {
                return None;
            }
            KeyCode::Char(if m.shift { c.to_ascii_uppercase() } else { c })
        }
    };
    Some(Key { code, mods })
}

#[derive(Clone, Copy)]
pub(crate) struct HeightHandle;

impl Shell {
    /// Whether the pointer rests on a link of the terminal: Ctrl pressed or
    /// released there draws it as one, or not.
    pub(crate) fn terminal_pointer_over_link(&self) -> bool {
        self.terminal.open && self.terminal.grid.pointer_on_link()
    }

    fn terminal_session(&self) -> Option<Arc<dyn TerminalSession>> {
        self.terminal.session.clone()
    }

    /// F4: show the terminal, starting a shell in the active pane's folder
    /// the first time, and give it the keyboard; or put it away.
    pub(crate) fn toggle_terminal(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.terminal.open {
            self.terminal.open = false;
            self.terminal.label(None);
            // The panel leaves with the focus: the pane takes it back
            // (`Shell::drawn_focus`).
        } else if self.app.terminals.provider().is_none() {
            self.toast(
                crate::toast::Toast {
                    text: "The terminal is a plugin, and it is off".into(),
                    tone: crate::toast::Tone::Error,
                    action: Some(crate::toast::Action {
                        label: "Open plugins".into(),
                        command: "ui.settings".into(),
                        args: serde_json::json!({ "page": "plugins" }),
                    }),
                },
                cx,
            );
            return;
        } else {
            self.terminal.open = true;
            window.focus(&self.terminal.focus);
            self.start_terminal(window, cx);
        }
        self.remember_session();
        cx.notify();
    }

    /// A shell for the open panel, if it has none and none is starting.
    fn start_terminal(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.terminal.session.is_some() || self.terminal.opening {
            return;
        }
        let Some(provider) = self.app.terminals.provider() else {
            return;
        };
        let dir = self
            .app
            .pane_dir(self.pane)
            .ok()
            .or_else(|| std::env::var_os("HOME").map(PathBuf::from))
            .unwrap_or_else(|| PathBuf::from("/"));
        let theme = cx.global::<Theme>();
        let spawn = Spawn {
            dir: dir.clone(),
            // Until the grid has measured itself, a terminal's usual size.
            size: Size {
                cols: 80,
                rows: 24,
                cell_width: 8,
                cell_height: 16,
            },
            palette: palette(theme),
        };
        let waker = self.app.waker_handle();
        self.terminal.opening = true;
        tracing::debug!("terminal: starting a shell");
        self.off_thread(
            "terminal-open",
            move || provider.open(spawn, waker),
            move |this, result, window, cx| {
                this.terminal.opening = false;
                match result {
                    // Closed while it started: the shell is not wanted.
                    Ok(_) if !this.terminal.open => {
                        tracing::debug!("terminal: closed while its shell started");
                    }
                    Ok(session) => {
                        this.terminal.forget_session();
                        this.terminal.followed = Some(dir);
                        this.terminal.session = Some(session);
                        this.sync_terminal(window, cx);
                    }
                    Err(error) => {
                        this.terminal.open = false;
                        this.remember_session();
                        this.set_status(format!("The terminal did not start: {error:#}"), true, cx);
                    }
                }
                cx.notify();
            },
            window,
            cx,
        );
    }

    /// Apply what the session published and tell it what changed here.
    /// Cheap when nothing did: two atomic loads and a comparison.
    pub(crate) fn sync_terminal(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let providers = self.app.terminals.revision();
        if providers != self.terminal.provider_revision {
            self.terminal.provider_revision = providers;
            if self.app.terminals.provider().is_none() {
                tracing::debug!("terminal: the plugin is gone");
                // The plugin was disabled: its shells were hung up on.
                self.terminal.forget_session();
                if self.terminal.open {
                    self.terminal.open = false;
                    self.remember_session();
                }
            }
        }
        let Some(session) = self.terminal_session() else {
            // Restored open from the last window: start its shell.
            if self.terminal.open {
                self.start_terminal(window, cx);
            }
            return;
        };
        let revision = session.revision();
        if revision != self.terminal.revision {
            self.terminal.revision = revision;
            let screen = session.screen();
            if screen.exited {
                tracing::debug!("terminal: the shell ended");
                self.terminal.forget_session();
                self.terminal.open = false;
                self.remember_session();
                cx.notify();
                return;
            }
            // Selecting is copying to the desktop's primary selection, as
            // in every terminal on Linux; the middle button pastes it.
            if screen.selected_text != self.terminal.screen.selected_text
                && let Some(text) = &screen.selected_text
            {
                cx.write_to_primary(ClipboardItem::new_string(text.clone()));
            }
            if screen.moved != self.terminal.moved {
                self.terminal.moved = screen.moved;
                if let Some(cwd) = screen.cwd.clone()
                    && self.app.pane_dir(self.pane).ok().as_ref() != Some(&cwd)
                {
                    self.terminal.followed = Some(cwd.clone());
                    if let Err(error) = self
                        .app
                        .navigate(self.pane, &bibli_core::Location::from_path(&cwd))
                    {
                        self.set_status(format!("{error:#}"), true, cx);
                    }
                }
            }
            // Labels name the links shown: none left, none to type.
            if screen.links.is_empty() {
                self.terminal.label(None);
            }
            self.terminal.screen = screen;
            cx.notify();
        }
        if let Ok(dir) = self.app.pane_dir(self.pane)
            && self.terminal.followed.as_ref() != Some(&dir)
        {
            self.terminal.followed = Some(dir.clone());
            session.send(Input::Follow(dir));
        }
        let colors = palette(cx.global::<Theme>());
        if self.terminal.palette != Some(colors) {
            self.terminal.palette = Some(colors);
            session.send(Input::Palette(colors));
        }
    }

    pub(crate) fn terminal_copy(&mut self, cx: &mut Context<Self>) {
        if let Some(text) = &self.terminal.screen.selected_text {
            cx.write_to_clipboard(ClipboardItem::new_string(text.clone()));
        }
    }

    pub(crate) fn terminal_paste(&mut self, cx: &mut Context<Self>) {
        let (Some(session), Some(text)) = (
            self.terminal_session(),
            cx.read_from_clipboard().and_then(|item| item.text()),
        ) else {
            return;
        };
        if !text.is_empty() {
            session.send(Input::Paste(text));
        }
    }

    pub(crate) fn terminal_page(&mut self, args: &serde_json::Value) {
        let pages = args
            .get("pages")
            .and_then(serde_json::Value::as_i64)
            .unwrap_or(0);
        if let Some(session) = self.terminal_session() {
            session.send(Input::Page(i32::try_from(pages).unwrap_or(0)));
        }
    }

    /// `ui.terminal_links`: label the links on screen, so typing a label
    /// follows its link; again, or Escape, stops.
    pub(crate) fn label_terminal_links(&mut self, cx: &mut Context<Self>) {
        if self.terminal.labelled.is_some() {
            self.terminal.label(None);
        } else if self.terminal.screen.links.is_empty() {
            self.set_status("No link on the terminal's screen", false, cx);
        } else {
            self.terminal.label(Some(String::new()));
        }
        cx.notify();
    }

    /// A key while the links are labelled: a letter of a label (a capital
    /// for a tab of its own), Backspace, or Escape. Nothing reaches the
    /// shell.
    fn terminal_label_key(
        &mut self,
        event: &KeyDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(mut typed) = self.terminal.labelled.clone() else {
            return;
        };
        let keystroke = &event.keystroke;
        match keystroke.key.as_str() {
            "escape" => return self.terminal.label(None),
            "backspace" => {
                typed.pop();
                return self.terminal.label(Some(typed));
            }
            _ => {}
        }
        let letter = keystroke
            .key_char
            .as_deref()
            .unwrap_or(&keystroke.key)
            .chars()
            .next()
            .filter(char::is_ascii_alphabetic);
        let Some(letter) = letter else {
            return;
        };
        let tab = keystroke.modifiers.shift || letter.is_ascii_uppercase();
        typed.push(letter.to_ascii_lowercase());
        let labels = crate::links::labels(self.terminal.screen.links.len());
        if let Some(index) = labels.iter().position(|label| *label == typed) {
            let target = self.terminal.screen.links[index].target.clone();
            self.terminal.label(None);
            self.follow_terminal_link(&target, tab, window, cx);
        } else if labels.iter().any(|label| label.starts_with(&typed)) {
            self.terminal.label(Some(typed));
        } else {
            self.terminal.label(None);
            self.set_status("No link has that label", false, cx);
        }
    }

    fn on_terminal_key(
        &mut self,
        event: &KeyDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.terminal.labelled.is_some() {
            self.terminal_label_key(event, window, cx);
            cx.stop_propagation();
            cx.notify();
            return;
        }
        let Some(session) = self.terminal_session() else {
            return;
        };
        if let Some(key) = key_of(&event.keystroke) {
            session.send(Input::Key(key));
            cx.stop_propagation();
        }
    }

    /// The panel's top edge follows the pointer; its foot is the bottom of
    /// the column it closes, whose bounds GPUI hands the drag.
    pub(crate) fn on_terminal_height_drag(
        &mut self,
        event: &DragMoveEvent<HeightHandle>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let foot = f32::from(event.bounds.bottom());
        let pointer = f32::from(event.event.position.y);
        self.terminal.height =
            Some((foot - pointer).clamp(MIN_HEIGHT, self.terminal_height_limit(window)));
        cx.notify();
    }

    /// The tallest the panel may be: the window, less what the stacked
    /// panes keep, the bars above and below, and the preview at the
    /// bottom.
    pub(crate) fn terminal_height_limit(&self, window: &Window) -> f32 {
        let preview = if self.preview.open && self.layout.bottom() {
            self.preview_panel_height()
        } else {
            0.
        };
        (crate::preview::max_height(f32::from(window.viewport_size().height), self.panes_down())
            - preview)
            .max(MIN_HEIGHT)
    }

    /// The panel, or `None` when it is put away.
    pub(crate) fn render_terminal(
        &self,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        if !self.terminal.open {
            return None;
        }
        let theme = cx.global::<Theme>().clone();
        let height = self
            .terminal
            .height
            .unwrap_or(DEFAULT_HEIGHT)
            .min(self.terminal_height_limit(window));
        let weak = cx.entity().downgrade();
        let follow: crate::elements::terminal::FollowLink =
            Rc::new(move |target, tab, window, cx| {
                let _ = weak.update(cx, |shell, cx| {
                    shell.note_pointer_use("ui.terminal_links", &serde_json::json!({}), cx);
                    shell.follow_terminal_link(target, tab, window, cx);
                });
            });
        let labels = self.terminal.labelled.clone().map(|typed| {
            (
                crate::links::labels(self.terminal.screen.links.len()),
                typed,
            )
        });
        let grid = self.terminal_session().map(|session| {
            let mut font = gpui::font(self.mono_font.clone());
            // Glyphs sit one per cell: a ligature would shift the rest.
            font.features =
                gpui::FontFeatures(Arc::new(vec![("calt".into(), 0), ("liga".into(), 0)]));
            TerminalGrid {
                session,
                screen: Arc::clone(&self.terminal.screen),
                focus: self.terminal.focus.clone(),
                font,
                font_size: px(FONT_SIZE),
                colors: Colors {
                    fg: theme.text,
                    bg: theme.bg,
                    ansi: theme.terminal,
                    selection: theme.selection_bg,
                    link: theme.accent,
                    on_link: theme.on_fill,
                },
                state: Rc::clone(&self.terminal.grid),
                follow,
                labels,
            }
        });
        // Where a link a program wrote leads, said before it is followed:
        // its text may say something else.
        let tip = {
            let (grid, font) = (Rc::clone(&self.terminal.grid), self.font.clone());
            move |_: &mut Window, cx: &mut gpui::App| {
                grid.tip()
                    .map(|tip| crate::toolbar::tooltip(tip, font.clone(), cx))
            }
        };
        let reset = cx.listener(|this, event: &gpui::ClickEvent, _, cx| {
            if event.click_count() == 2 {
                this.terminal.height = None;
                this.remember_session();
                cx.notify();
            }
        });
        let edge = div()
            .id("terminal-resize")
            .absolute()
            .left_0()
            .top_0()
            .w_full()
            .h(px(6.))
            .cursor(CursorStyle::ResizeUpDown)
            .hover(|style| style.bg(theme.handle))
            .on_click(reset)
            .on_drag(HeightHandle, |_, _, _, cx| {
                cx.new(|_| crate::preview::NoDragView)
            });
        Some(
            div()
                .id("terminal-panel")
                .track_focus(&self.terminal.focus)
                .key_context(keymap::TERMINAL)
                .optional_tooltip(tip)
                .on_key_down(cx.listener(Self::on_terminal_key))
                .relative()
                .flex_none()
                .w_full()
                .h(px(height))
                .border_t_1()
                .border_color(theme.border)
                .bg(theme.bg)
                .cursor(CursorStyle::IBeam)
                .children(grid)
                .child(edge)
                .into_any_element(),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn stroke(text: &str) -> gpui::Keystroke {
        gpui::Keystroke::parse(text).unwrap()
    }

    #[test]
    fn keys_without_text_and_chords_are_the_terminals() {
        let key = |text: &str| key_of(&stroke(text)).map(|key| (key.code, key.mods));
        assert_eq!(key("enter").unwrap().0, KeyCode::Enter);
        assert_eq!(key("f12").unwrap().0, KeyCode::F(12));
        assert_eq!(key("f13"), None);
        let (code, mods) = key("ctrl-c").unwrap();
        assert_eq!((code, mods.ctrl), (KeyCode::Char('c'), true));
        assert_eq!(key("alt-b").unwrap().0, KeyCode::Char('b'));
        assert_eq!(key("ctrl-space").unwrap().0, KeyCode::Char(' '));
        assert_eq!(
            key("shift-tab").unwrap(),
            (
                KeyCode::Tab,
                Mods {
                    shift: true,
                    ..Mods::default()
                }
            )
        );
        // Text goes through the input handler, composed.
        assert_eq!(key("a"), None);
        assert_eq!(key("shift-a"), None);
        assert_eq!(key("space"), None);
        assert_eq!(key("cmd-c"), None);
    }
}
