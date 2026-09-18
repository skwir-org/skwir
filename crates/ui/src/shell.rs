//! The shell entity: owns the window's UI state, routes every command, and
//! wakes only when the core signals new events.

use std::sync::Arc;
use std::time::Instant;

use bibli_core::command::{Invocation, Outcome};
use bibli_core::op::StepStatus;
use bibli_core::workspace::PaneId;
use bibli_core::{App, Location};
use gpui::{
    Context, Entity, FocusHandle, IntoElement, SharedString, Subscription, Window, div, prelude::*,
};

use crate::input::{InputEvent, TextInput};
use crate::keymap::{self, KeyHint, RunCommand};
use crate::list::ListState;
use crate::menu::MenuState;
use crate::palette::PaletteState;
use crate::prompts::{ConfirmState, PromptKind, PromptState};
use crate::status::StatusState;
use crate::theme::Theme;
use crate::{address, list, menu, palette, prompts, sidebar, status};

pub struct Shell {
    pub app: Arc<App>,
    pub pane: PaneId,
    pub font: SharedString,
    pub key_hints: Vec<KeyHint>,
    pub pane_focus: FocusHandle,
    pub address: Entity<TextInput>,
    /// The location string the address bar currently mirrors.
    pub address_shown: String,
    pub list: ListState,
    pub palette: Option<PaletteState>,
    pub prompt: Option<PromptState>,
    pub confirm: Option<ConfirmState>,
    pub menu: Option<MenuState>,
    pub status: StatusState,
    /// Total listing events applied by `pump`; the list resorts when it changes.
    pub events_seen: u64,
    started: Instant,
    first_frame_logged: bool,
    _subscriptions: Vec<Subscription>,
}

impl Shell {
    pub fn new(
        app: Arc<App>,
        initial: Option<Location>,
        started: Instant,
        font: SharedString,
        key_hints: Vec<KeyHint>,
        wake_rx: flume::Receiver<()>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        cx.set_global(Theme::for_appearance(window.appearance()));
        let mut subs = Vec::new();
        subs.push(cx.observe_window_appearance(window, |_, window, cx| {
            cx.set_global(Theme::for_appearance(window.appearance()));
            cx.notify();
        }));

        let address = cx.new(|cx| TextInput::new("Path or location", cx));
        subs.push(cx.subscribe_in(
            &address,
            window,
            |this, input, ev: &InputEvent, window, cx| match ev {
                InputEvent::Submit => {
                    let text = input.read(cx).text().trim().to_owned();
                    window.focus(&this.pane_focus);
                    if !text.is_empty() {
                        this.run(
                            "navigate",
                            serde_json::json!({ "location": text }),
                            window,
                            cx,
                        );
                    }
                }
                InputEvent::Cancel => {
                    let shown = this.address_shown.clone();
                    input.update(cx, |i, cx| i.set_text(shown, cx));
                    window.focus(&this.pane_focus);
                    cx.notify();
                }
                InputEvent::Changed => {}
            },
        ));

        // Wake-up, not polling: the core signals, we notify, GPUI re-renders.
        cx.spawn(async move |this, cx| {
            loop {
                if wake_rx.recv_async().await.is_err() {
                    break;
                }
                while wake_rx.try_recv().is_ok() {}
                if this.update(cx, |_, cx| cx.notify()).is_err() {
                    break;
                }
            }
        })
        .detach();

        let pane = app.workspace.read().active_id();
        let mut shell = Shell {
            app,
            pane,
            font,
            key_hints,
            pane_focus: cx.focus_handle(),
            address,
            address_shown: String::new(),
            list: ListState::default(),
            palette: None,
            prompt: None,
            confirm: None,
            menu: None,
            status: StatusState::default(),
            events_seen: 0,
            started,
            first_frame_logged: false,
            _subscriptions: subs,
        };
        if let Some(loc) = initial
            && let Err(e) = shell.app.navigate(pane, &loc)
        {
            shell.set_status(format!("{e:#}"), true, cx);
        }
        shell
    }

    fn on_run_command(&mut self, a: &RunCommand, window: &mut Window, cx: &mut Context<Self>) {
        self.run(&a.name, a.args.clone(), window, cx);
    }

    /// Route a command: `ui.*` to the shell, everything else to the core.
    pub fn run(
        &mut self,
        name: &str,
        args: serde_json::Value,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        tracing::debug!(name, %args, "run");
        if let Some(ui) = name.strip_prefix("ui.") {
            self.run_ui(ui, window, cx);
            cx.notify();
            return;
        }
        if name == "open" && self.app.commands.get("open").is_none() {
            self.open_fallback(window, cx);
            return;
        }
        let inv = Invocation {
            name: name.into(),
            args,
        };
        match self.app.dispatch(&inv) {
            Ok(Outcome::Done | Outcome::Value(_)) => {}
            Ok(Outcome::Plan(plan)) => {
                if plan.is_applicable() {
                    self.app.apply(plan);
                } else {
                    let reason = plan
                        .diff()
                        .steps
                        .into_iter()
                        .find_map(|(_, s)| match s {
                            StepStatus::Conflict { reason } => Some(reason),
                            _ => None,
                        })
                        .unwrap_or_else(|| "plan cannot be applied".into());
                    self.set_status(reason, true, cx);
                }
            }
            Err(e) => self.set_status(format!("{e:#}"), true, cx),
        }
        cx.notify();
    }

    fn run_ui(&mut self, name: &str, window: &mut Window, cx: &mut Context<Self>) {
        match name {
            "open" => self.open_selection(window, cx),
            "focus_address" => {
                self.address.update(cx, |i, cx| {
                    i.select_all_now(cx);
                    i.focus(window);
                });
            }
            "focus_pane" => window.focus(&self.pane_focus),
            "palette" => self.open_palette(window, cx),
            "palette_up" => self.palette_move(-1, cx),
            "palette_down" => self.palette_move(1, cx),
            "rename_prompt" => {
                let single = {
                    let ws = self.app.workspace.read();
                    ws.pane(self.pane).and_then(|p| {
                        let mut it = p.selection.iter();
                        let id = it.next()?;
                        if it.next().is_some() {
                            return None;
                        }
                        p.listing.get(id).map(|e| e.name_str().into_owned())
                    })
                };
                match single {
                    Some(name) => {
                        let stem = name.rfind('.').filter(|&i| i > 0).unwrap_or(name.len());
                        self.open_prompt(
                            PromptKind::Rename,
                            "Rename",
                            &name,
                            Some(0..stem),
                            window,
                            cx,
                        );
                    }
                    None => self.set_status("select exactly one entry to rename", true, cx),
                }
            }
            "mkdir_prompt" => {
                self.open_prompt(PromptKind::Mkdir, "New folder", "", None, window, cx);
            }
            "confirm_delete" => {
                let n = self.selection_ctx().count;
                if n == 0 {
                    self.set_status("nothing selected", true, cx);
                } else {
                    let noun = if n == 1 { "item" } else { "items" };
                    self.open_confirm(
                        format!("Delete {n} {noun} permanently? This cannot be undone."),
                        Invocation::new("delete"),
                        window,
                        cx,
                    );
                }
            }
            "accept" => self.confirm_accept(window, cx),
            "dismiss" => self.confirm_dismiss(window, cx),
            "toggle_hidden" => self.list.show_hidden = !self.list.show_hidden,
            "cursor_up" => self.cursor_move(-1, false, window, cx),
            "cursor_down" => self.cursor_move(1, false, window, cx),
            "cursor_extend_up" => self.cursor_move(-1, true, window, cx),
            "cursor_extend_down" => self.cursor_move(1, true, window, cx),
            "cursor_home" => self.cursor_to(0, false, window, cx),
            "cursor_end" => self.cursor_to(isize::MAX, false, window, cx),
            "cursor_extend_home" => self.cursor_to(0, true, window, cx),
            "cursor_extend_end" => self.cursor_to(isize::MAX, true, window, cx),
            "page_up" => {
                let n = self.list.visible_rows.saturating_sub(1).max(1) as isize;
                self.cursor_move(-n, false, window, cx);
            }
            "page_down" => {
                let n = self.list.visible_rows.saturating_sub(1).max(1) as isize;
                self.cursor_move(n, false, window, cx);
            }
            "quit" => cx.quit(),
            other => self.set_status(format!("unknown ui command `ui.{other}`"), true, cx),
        }
    }

    /// `open` through the open-with plugin, or the built-in fallback.
    pub fn open_selection(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.app.commands.get("open").is_some() {
            self.run("open", serde_json::json!({}), window, cx);
        } else {
            self.open_fallback(window, cx);
        }
    }

    /// Without an `open` command: enter a single selected directory, ignore files.
    fn open_fallback(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let target = {
            let ws = self.app.workspace.read();
            ws.pane(self.pane).and_then(|p| {
                let mut it = p.selection.iter();
                let id = it.next()?;
                if it.next().is_some() {
                    return None;
                }
                let e = p.listing.get(id)?;
                if !e.is_dir() {
                    return None;
                }
                p.open.as_ref()?.path_of(id)
            })
        };
        if let Some(path) = target {
            self.run(
                "navigate",
                serde_json::json!({ "location": path.to_string_lossy() }),
                window,
                cx,
            );
        }
    }

    /// Apply core events and refresh derived state. Called first in `render`.
    fn sync(&mut self, cx: &mut Context<Self>) {
        let t0 = Instant::now();
        let applied = self.app.pump();
        self.events_seen += applied as u64;
        self.drain_executor(cx);
        let before = self.list.sorted_version();
        let location = {
            let ws = self.app.workspace.read();
            let Some(pane) = ws.pane(self.pane) else {
                return;
            };
            let location = pane.location().map(ToString::to_string).unwrap_or_default();
            if location != self.address_shown {
                self.list.reset();
            }
            self.list.sync(&pane.listing, self.events_seen);
            location
        };
        if location != self.address_shown {
            self.address_shown.clone_from(&location);
            self.address.update(cx, |i, cx| i.set_text(location, cx));
        }
        let resorted = self.list.sorted_version() != before;
        if applied > 0 || resorted {
            tracing::debug!(
                applied,
                resorted,
                rows = self.list.sorted.len(),
                took_us = t0.elapsed().as_micros() as u64,
                "sync"
            );
        }
    }
}

impl Render for Shell {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if !self.first_frame_logged {
            self.first_frame_logged = true;
            tracing::info!(
                "first frame {:.1} ms after start",
                self.started.elapsed().as_secs_f64() * 1000.0
            );
        }
        self.sync(cx);
        let theme = cx.global::<Theme>().clone();
        let address_bar = address::render(self, window, cx);
        let side = sidebar::render(self, cx);
        let file_list = list::render(self, cx);
        let status_bar = status::render(self, cx);
        let overlays = [
            menu::render(self, cx),
            prompts::render_prompt(self, cx),
            prompts::render_confirm(self, cx),
            palette::render(self, cx),
        ];

        div()
            .key_context(keymap::ROOT)
            .on_action(cx.listener(Self::on_run_command))
            .relative()
            .size_full()
            .flex()
            .flex_col()
            .font_family(self.font.clone())
            .text_sm()
            .bg(theme.bg)
            .text_color(theme.text)
            .child(address_bar)
            .child(
                div()
                    .flex()
                    .flex_row()
                    .flex_1()
                    .min_h_0()
                    .w_full()
                    .child(side)
                    .child(file_list),
            )
            .child(status_bar)
            .children(overlays.into_iter().flatten())
    }
}
