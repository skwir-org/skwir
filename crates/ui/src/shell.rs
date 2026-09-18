//! The shell entity: owns the window's UI state (per pane and global),
//! routes every command, and wakes only when the core signals new events.

use std::collections::{HashMap, VecDeque};
use std::sync::Arc;
use std::time::Instant;

use bibli_core::app::NotifyLevel;
use bibli_core::command::{Invocation, Outcome};
use bibli_core::op::StepStatus;
use bibli_core::workspace::PaneId;
use bibli_core::{App, Location, Plan, SortKey};
use gpui::{
    ClipboardItem, Context, Entity, FocusHandle, IntoElement, SharedString, Subscription, Window,
    div, prelude::*, px,
};

use crate::icons::IconCache;
use crate::input::{InputEvent, TextInput};
use crate::jump::JumpState;
use crate::keymap::{self, KeyHint, RunCommand};
use crate::list::ListState;
use crate::menu::MenuState;
use crate::palette::PaletteState;
use crate::preview::PreviewState;
use crate::prompts::{ConfirmState, PromptKind, PromptState};
use crate::renamebulk::BulkState;
use crate::status::StatusState;
use crate::theme::Theme;
use crate::thumbs::ThumbCache;
use crate::timeline::TimelineState;
use crate::usage::UsageState;
use crate::view::{self, Nav, PaneView};
use crate::views::grid::ZOOMS;
use crate::{
    address, jump, menu, palette, preview, prompts, renamebulk, review, sidebar, status, timeline,
    usage,
};

/// UI state of one pane: focus, address bar, cached order, views.
pub struct PaneUi {
    pub id: PaneId,
    pub focus: FocusHandle,
    pub address: Entity<TextInput>,
    /// The location string the address bar currently mirrors.
    pub address_shown: String,
    pub list: ListState,
    /// Instantiated views by id; the active one is taken out during render.
    pub views: HashMap<&'static str, Box<dyn PaneView>>,
    pub view_id: &'static str,
    pub grid_rows_visible: usize,
    _sub: Subscription,
}

pub struct Shell {
    pub app: Arc<App>,
    /// The pane commands act on: the workspace's active pane, or the pane
    /// being rendered while a pane renders.
    pub pane: PaneId,
    pub panes: Vec<PaneUi>,
    pub font: SharedString,
    pub mono_font: SharedString,
    pub key_hints: Vec<KeyHint>,
    pub zoom: u32,
    pub thumbs: ThumbCache,
    pub icons: IconCache,
    pub preview: PreviewState,
    pub palette: Option<PaletteState>,
    pub jump: Option<JumpState>,
    pub bulk: Option<BulkState>,
    pub prompt: Option<PromptState>,
    pub confirm: Option<ConfirmState>,
    pub menu: Option<MenuState>,
    /// Plans proposed by background work, oldest first; the front is shown.
    pub reviews: VecDeque<Plan>,
    pub review_focus: Option<FocusHandle>,
    pub usage: UsageState,
    pub timeline: TimelineState,
    pub status: StatusState,
    pub last_search: String,
    pub shelf_count: usize,
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
        mono_font: SharedString,
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
            panes: Vec::new(),
            font,
            mono_font,
            key_hints,
            zoom: 128,
            thumbs: ThumbCache::default(),
            icons: IconCache::default(),
            preview: PreviewState::default(),
            palette: None,
            jump: None,
            bulk: None,
            prompt: None,
            confirm: None,
            menu: None,
            reviews: VecDeque::new(),
            review_focus: None,
            usage: UsageState::default(),
            timeline: TimelineState::default(),
            status: StatusState::default(),
            last_search: String::new(),
            shelf_count: 0,
            started,
            first_frame_logged: false,
            _subscriptions: subs,
        };
        let ui = shell.make_pane_ui(pane, window, cx);
        shell.panes.push(ui);
        if let Some(loc) = initial
            && let Err(e) = shell.app.navigate(pane, &loc)
        {
            shell.set_status(format!("{e:#}"), true, cx);
        }
        shell
    }

    fn make_pane_ui(&self, id: PaneId, window: &mut Window, cx: &mut Context<Self>) -> PaneUi {
        let address = cx.new(|cx| TextInput::new("Path, query:… or location", cx));
        let sub = cx.subscribe_in(
            &address,
            window,
            move |this, input, ev: &InputEvent, window, cx| match ev {
                InputEvent::Submit => {
                    let text = input.read(cx).text().trim().to_owned();
                    this.activate_pane(id, cx);
                    window.focus(&this.ui().focus);
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
                    this.activate_pane(id, cx);
                    let shown = this.ui().address_shown.clone();
                    input.update(cx, |i, cx| i.set_text(shown, cx));
                    window.focus(&this.ui().focus);
                    cx.notify();
                }
                InputEvent::Changed => {}
            },
        );
        let mut views: HashMap<&'static str, Box<dyn PaneView>> = HashMap::new();
        for spec in self.app.registry.views.all() {
            if let Some(v) = view::instantiate(&spec) {
                views.insert(v.id(), v);
            }
        }
        PaneUi {
            id,
            focus: cx.focus_handle(),
            address,
            address_shown: String::new(),
            list: ListState::default(),
            views,
            view_id: view::LIST,
            grid_rows_visible: 4,
            _sub: sub,
        }
    }

    // ------------------------------------------------------------ layout

    // ------------------------------------------------------------ panes

    pub fn ui(&self) -> &PaneUi {
        self.panes
            .iter()
            .find(|p| p.id == self.pane)
            .or_else(|| self.panes.first())
            .expect("at least one pane")
    }

    pub fn ui_mut(&mut self) -> &mut PaneUi {
        let pane = self.pane;
        let ix = self.panes.iter().position(|p| p.id == pane).unwrap_or(0);
        &mut self.panes[ix]
    }

    /// Run `f` with `pane` as the pane commands act on (processors run at
    /// layout time, after `render_pane` restored the active pane).
    pub fn with_pane<R>(&mut self, pane: PaneId, f: impl FnOnce(&mut Self) -> R) -> R {
        let saved = self.pane;
        self.pane = pane;
        let r = f(self);
        self.pane = saved;
        r
    }

    pub fn is_split(&self) -> bool {
        self.panes.len() > 1
    }

    /// The other pane when split.
    pub fn other_pane(&self) -> Option<PaneId> {
        self.panes.iter().map(|p| p.id).find(|id| *id != self.pane)
    }

    /// Make `id` the active pane (workspace and shell agree).
    pub fn activate_pane(&mut self, id: PaneId, cx: &mut Context<Self>) {
        if self.app.workspace.read().active_id() == id && self.pane == id {
            return;
        }
        self.app.workspace.write().set_active(id);
        self.pane = id;
        cx.notify();
    }

    fn split(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.is_split() {
            // Close the other pane; keep the active one.
            let other = self.other_pane();
            if let Some(o) = other {
                self.app.workspace.write().close_pane(o);
                self.panes.retain(|p| p.id != o);
            }
            window.focus(&self.ui().focus);
            cx.notify();
            return;
        }
        let here = {
            let ws = self.app.workspace.read();
            ws.pane(self.pane).and_then(|p| p.location().cloned())
        };
        let id = self.app.workspace.write().add_pane();
        let ui = self.make_pane_ui(id, window, cx);
        self.panes.push(ui);
        if let Some(loc) = here
            && let Err(e) = self.app.navigate(id, &loc)
        {
            self.set_status(format!("{e:#}"), true, cx);
        }
        cx.notify();
    }

    fn focus_other_pane(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(o) = self.other_pane() {
            self.activate_pane(o, cx);
            window.focus(&self.ui().focus);
        }
    }

    /// Directory shown by the other pane, for F5/F6 and the shelf buttons.
    pub fn other_pane_dir(&self) -> Option<std::path::PathBuf> {
        let other = self.other_pane()?;
        let ws = self.app.workspace.read();
        ws.pane(other)
            .and_then(|p| p.location().and_then(Location::path))
    }

    fn send_to_other_pane(&mut self, cmd: &str, window: &mut Window, cx: &mut Context<Self>) {
        match self.other_pane_dir() {
            Some(dest) => self.run(
                cmd,
                serde_json::json!({ "dest": dest.to_string_lossy() }),
                window,
                cx,
            ),
            None => self.set_status("the other pane is not showing a directory", true, cx),
        }
    }

    // ------------------------------------------------------------ views

    pub fn take_view(&mut self) -> Option<Box<dyn PaneView>> {
        let id = self.ui().view_id;
        self.ui_mut().views.remove(id)
    }

    pub fn put_view(&mut self, v: Box<dyn PaneView>) {
        self.ui_mut().views.insert(v.id(), v);
    }

    pub fn with_view<R>(&mut self, f: impl FnOnce(&mut dyn PaneView, &mut Self) -> R) -> Option<R> {
        let mut v = self.take_view()?;
        let r = f(v.as_mut(), self);
        self.put_view(v);
        Some(r)
    }

    fn set_view(&mut self, id: &str, cx: &mut Context<Self>) {
        let known = self.ui().views.keys().find(|k| **k == id).copied();
        match known {
            Some(k) => {
                self.ui_mut().view_id = k;
                let ix = self.cursor_index();
                self.with_view(|v, _| {
                    if let Some(ix) = ix {
                        v.scroll_to(ix);
                    }
                });
                cx.notify();
            }
            None => self.set_status(format!("unknown view `{id}`"), true, cx),
        }
    }

    fn zoom_step(&mut self, dir: &str, cx: &mut Context<Self>) {
        let pos = ZOOMS.iter().position(|z| *z == self.zoom).unwrap_or(1);
        self.zoom = match dir {
            "in" => ZOOMS[(pos + 1).min(ZOOMS.len() - 1)],
            "out" => ZOOMS[pos.saturating_sub(1)],
            _ => 128,
        };
        let ix = self.cursor_index();
        self.with_view(|v, _| {
            if let Some(ix) = ix {
                v.scroll_to(ix);
            }
        });
        cx.notify();
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
            self.run_ui(ui, &args, window, cx);
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
            Ok(Outcome::Done) => {}
            Ok(Outcome::Value(v)) => self.on_value(name, v, cx),
            Ok(Outcome::Plan(plan)) => self.apply_user_plan(plan, cx),
            Err(e) => self.set_status(format!("{e:#}"), true, cx),
        }
        cx.notify();
    }

    /// A user-initiated plan applies immediately (it is undoable); conflicts
    /// are reported instead.
    pub fn apply_user_plan(&mut self, plan: Plan, cx: &mut Context<Self>) {
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

    /// Values from commands the shell knows how to present; others are ignored.
    fn on_value(&mut self, name: &str, v: serde_json::Value, cx: &mut Context<Self>) {
        if name == "shell.copy_paths"
            && let Some(s) = v.as_str()
        {
            let n = self.selection_ctx().count;
            cx.write_to_clipboard(ClipboardItem::new_string(s.to_owned()));
            self.set_status(
                format!("Copied {n} path{}", if n == 1 { "" } else { "s" }),
                false,
                cx,
            );
        }
    }

    /// Dispatch and hand back a `Value` (for plugin queries the UI renders).
    pub fn query_value(
        &mut self,
        name: &str,
        args: serde_json::Value,
        cx: &mut Context<Self>,
    ) -> Option<serde_json::Value> {
        match self.app.dispatch(&Invocation {
            name: name.into(),
            args,
        }) {
            Ok(Outcome::Value(v)) => Some(v),
            Ok(_) => None,
            Err(e) => {
                self.set_status(format!("{e:#}"), true, cx);
                None
            }
        }
    }

    fn run_ui(
        &mut self,
        name: &str,
        args: &serde_json::Value,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let arg = |k: &str| args.get(k).and_then(|v| v.as_str()).map(str::to_owned);
        match name {
            "open" => self.open_selection(window, cx),
            "focus_address" => {
                let address = self.ui().address.clone();
                address.update(cx, |i, cx| {
                    i.select_all_now(cx);
                    i.focus(window);
                });
            }
            "focus_pane" => window.focus(&self.ui().focus),
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
            "search" => {
                let last = self.last_search.clone();
                self.open_prompt(
                    PromptKind::Search,
                    "Search (e.g. *.pdf modified<7d size>10M)",
                    &last,
                    None,
                    window,
                    cx,
                );
            }
            "tags_prompt" => {
                let current = self.selection_tags().join(", ");
                self.open_prompt(
                    PromptKind::Tags,
                    "Tags (comma-separated)",
                    &current,
                    None,
                    window,
                    cx,
                );
            }
            "jump" => self.open_jump(window, cx),
            "jump_up" => self.jump_move(-1, cx),
            "jump_down" => self.jump_move(1, cx),
            "rename_bulk_prompt" => self.open_bulk(window, cx),
            "rename_bulk_next" => self.bulk_focus_next(1, window, cx),
            "rename_bulk_prev" => self.bulk_focus_next(-1, window, cx),
            "rename_bulk_case" => self.bulk_cycle_case(cx),
            "review_apply" => self.review_apply(cx),
            "review_cancel" => self.review_cancel(cx),
            "usage" => self.open_usage(window, cx),
            "usage_close" => self.close_usage(cx),
            "timeline" => self.toggle_timeline(cx),
            "timeline_bucket" => {
                let b = arg("bucket").unwrap_or_else(|| "month".into());
                self.timeline_set_bucket(&b, cx);
            }
            "timeline_subtree" => self.timeline_toggle_subtree(cx),
            "split" => self.split(window, cx),
            "focus_other_pane" => self.focus_other_pane(window, cx),
            "copy_other" => self.send_to_other_pane("copy", window, cx),
            "move_other" => self.send_to_other_pane("move", window, cx),
            "shelf_open" => self.run(
                "navigate",
                serde_json::json!({ "location": "shelf:" }),
                window,
                cx,
            ),
            "shelf_move_here" | "shelf_copy_here" => {
                let cmd = if name == "shelf_move_here" {
                    "shelf.move_here"
                } else {
                    "shelf.copy_here"
                };
                match self.other_pane() {
                    Some(other) => self.run(cmd, serde_json::json!({ "pane": other }), window, cx),
                    None => {
                        self.set_status("split the window to choose a destination pane", true, cx);
                    }
                }
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
            "toggle_hidden" => self.toggle_hidden(cx),
            "sort" => {
                let key = match arg("key").as_deref() {
                    Some("size") => SortKey::Size,
                    Some("modified") => SortKey::Modified,
                    _ => SortKey::Name,
                };
                self.sort_by(key, cx);
            }
            "view" => {
                let id = arg("id").unwrap_or_else(|| view::LIST.into());
                self.set_view(&id, cx);
            }
            "zoom" => {
                let dir = arg("dir").unwrap_or_else(|| "reset".into());
                self.zoom_step(&dir, cx);
            }
            "toggle_preview" => self.toggle_preview(cx),
            "peek" => self.toggle_peek(window, cx),
            "peek_close" => self.close_peek(window, cx),
            "peek_prev" => self.cursor_move(-1, false, window, cx),
            "peek_next" => self.cursor_move(1, false, window, cx),
            "cursor_up" => self.navigate_by(Nav::Up, false, window, cx),
            "cursor_down" => self.navigate_by(Nav::Down, false, window, cx),
            "cursor_left" => self.navigate_by(Nav::Left, false, window, cx),
            "cursor_right" => self.navigate_by(Nav::Right, false, window, cx),
            "cursor_extend_up" => self.navigate_by(Nav::Up, true, window, cx),
            "cursor_extend_down" => self.navigate_by(Nav::Down, true, window, cx),
            "cursor_extend_left" => self.navigate_by(Nav::Left, true, window, cx),
            "cursor_extend_right" => self.navigate_by(Nav::Right, true, window, cx),
            "cursor_home" => self.navigate_by(Nav::Home, false, window, cx),
            "cursor_end" => self.navigate_by(Nav::End, false, window, cx),
            "cursor_extend_home" => self.navigate_by(Nav::Home, true, window, cx),
            "cursor_extend_end" => self.navigate_by(Nav::End, true, window, cx),
            "page_up" => self.navigate_by(Nav::PageUp, false, window, cx),
            "page_down" => self.navigate_by(Nav::PageDown, false, window, cx),
            "quit" => {
                let (decoded, skipped, bytes) = self.app.thumbnails.stats();
                tracing::info!(decoded, skipped, cached_bytes = bytes, "thumbnail stats");
                cx.quit();
            }
            other => self.set_status(format!("unknown ui command `ui.{other}`"), true, cx),
        }
    }

    /// Union of the selection's tags, from the `tags` metadata field.
    pub fn selection_tags(&self) -> Vec<String> {
        let ws = self.app.workspace.read();
        let Some(pane) = ws.pane(self.pane) else {
            return Vec::new();
        };
        let mut out: Vec<String> = Vec::new();
        for id in pane.selection.iter() {
            if let Some(e) = pane.listing.get(id) {
                for t in crate::views::list::tags_of(e) {
                    if !out.contains(&t) {
                        out.push(t);
                    }
                }
            }
        }
        out
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
        let report = self.app.pump_report();
        self.app.thumbnails.tick();
        self.drain_executor(cx);
        for n in self.app.take_notifications() {
            let error = matches!(n.level, NotifyLevel::Error | NotifyLevel::Warning);
            self.set_status_for(n.text, error, if error { 8 } else { 4 }, cx);
        }
        let proposals = self.app.take_proposals();
        if !proposals.is_empty() {
            self.reviews.extend(proposals);
            cx.notify();
        }
        // Follow the workspace's active pane (commands may change it).
        let active = self.app.workspace.read().active_id();
        if self.panes.iter().any(|p| p.id == active) {
            self.pane = active;
        }
        let saved = self.pane;
        let ids: Vec<PaneId> = self.panes.iter().map(|p| p.id).collect();
        let mut resorted_any = false;
        for id in ids {
            self.pane = id;
            let app = Arc::clone(&self.app);
            let (location, resorted) = {
                let ws = app.workspace.read();
                let Some(pane) = ws.pane(id) else { continue };
                let location = pane.location().map(ToString::to_string).unwrap_or_default();
                let ui = self.ui_mut();
                if location != ui.address_shown {
                    ui.list.reset();
                }
                let resorted = ui.list.sync(&pane.listing, pane.sort);
                (location, resorted)
            };
            resorted_any |= resorted;
            if location != self.ui().address_shown {
                self.ui_mut().address_shown.clone_from(&location);
                let address = self.ui().address.clone();
                address.update(cx, |i, cx| i.set_text(location, cx));
                for v in self.ui_mut().views.values_mut() {
                    v.reset();
                }
            }
        }
        self.pane = saved;
        if report.applied > 0 || resorted_any {
            tracing::debug!(
                applied = report.applied,
                resorted = resorted_any,
                rows = self.ui().list.sorted.len(),
                took_us = t0.elapsed().as_micros() as u64,
                "sync"
            );
        }
        self.sync_preview(cx);
        self.sync_usage(cx);
        self.sync_timeline(cx);
        self.sync_shelf(cx);
    }

    fn sync_shelf(&mut self, cx: &mut Context<Self>) {
        if self.app.commands.get("shelf.list").is_none() {
            return;
        }
        let n = self
            .query_value("shelf.list", serde_json::json!({}), cx)
            .and_then(|v| v.as_array().map(Vec::len))
            .unwrap_or(0);
        if n != self.shelf_count {
            self.shelf_count = n;
            cx.notify();
        }
    }

    /// One pane column: its address bar and its view (or the usage panel).
    fn render_pane(
        &mut self,
        id: PaneId,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> gpui::AnyElement {
        let saved = self.pane;
        self.pane = id;
        let theme = cx.global::<Theme>().clone();
        let active = self.app.workspace.read().active_id() == id;
        let address_bar = address::render_address(self, window, cx);
        let body = if self.usage.active && active {
            usage::render(self, cx)
        } else {
            match self.take_view() {
                Some(mut v) => {
                    let el = v.render(self, window, cx);
                    self.put_view(v);
                    el
                }
                None => div()
                    .flex_1()
                    .child("no view registered")
                    .into_any_element(),
            }
        };
        let file_area = self.pane_container(body, cx);
        let strip = if active {
            timeline::render(self, cx)
        } else {
            None
        };
        self.pane = saved;
        div()
            .flex_1()
            .min_w_0()
            .h_full()
            .flex()
            .flex_col()
            .when(self.is_split(), |d| {
                d.border_l_1()
                    .border_color(if active { theme.accent } else { theme.border })
            })
            .child(address_bar)
            .child(file_area)
            .children(strip)
            .into_any_element()
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
        let toolbar = address::render_toolbar(self, cx);
        let side = sidebar::render(self, cx);
        let pane_ids: Vec<PaneId> = self.panes.iter().map(|p| p.id).collect();
        let mut columns = Vec::new();
        for id in pane_ids {
            columns.push(self.render_pane(id, window, cx));
        }
        // The review modal owns focus while a proposal waits.
        match (self.reviews.is_empty(), self.review_focus.is_some()) {
            (false, false) => {
                let h = cx.focus_handle();
                window.focus(&h);
                self.review_focus = Some(h);
            }
            (true, true) => {
                self.review_focus = None;
                window.focus(&self.ui().focus);
            }
            _ => {}
        }
        let panel = preview::render_panel(self, cx);
        let status_bar = status::render(self, cx);
        let overlays = [
            menu::render(self, cx),
            prompts::render_prompt(self, cx),
            prompts::render_confirm(self, cx),
            jump::render(self, cx),
            renamebulk::render(self, cx),
            palette::render(self, cx),
            preview::render_peek(self, cx),
            review::render(self, cx),
        ];

        let root = div()
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
            .child(toolbar)
            .child(
                div()
                    .flex()
                    .flex_row()
                    .flex_1()
                    .min_h_0()
                    .w_full()
                    .child(side)
                    .children(columns)
                    .children(panel),
            )
            .child(status_bar)
            .children(overlays.into_iter().flatten())
            .child(div().h(px(0.)));
        self.resize_hook(root, cx)
    }
}
