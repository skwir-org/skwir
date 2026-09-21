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
use crate::theme::Theme;
use crate::thumbs::ThumbCache;
use crate::timeline::TimelineState;
use crate::usage::UsageState;
use crate::view::{self, Nav, PaneView};
use crate::views::grid::ZOOMS;
use crate::{
    address, jump, menu, palette, preview, prompts, renamebulk, review, sidebar, status, timeline,
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
    view_registrations: HashMap<String, bibli_core::registry::Factory>,
    /// Registry revision the registrations above were reconciled against.
    views_revision: u64,
    pub view_id: &'static str,
    pub grid_rows_visible: usize,
    pub usage: UsageState,
    pub loading: crate::loading::State,
    _sub: Subscription,
}

/// A pending "show me this file": the pane it belongs to, the file name to
/// look for, and when to stop waiting for a listing that never brings it.
struct Reveal {
    pane: PaneId,
    name: std::ffi::OsString,
    until: Instant,
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
    pub type_names: crate::typenames::TypeNames,
    /// `ui.date_format`, resolved when the settings revision changes.
    pub friendly_dates: bool,
    /// Size, Modified, Type: which detail columns the list shows.
    /// Sidebar groups the user folded, for this session.
    pub sidebar_folded: std::collections::HashSet<String>,
    pub columns: [bool; 3],
    /// `ui.grid_fill`: thumbnails cropped to their box.
    pub grid_fill: bool,
    /// Ctrl and the wheel over the grid: a trackpad must mean it.
    pub zoom_wheel: crate::reader_scroll::WheelTurn,
    pub column_widths: [f32; 3],
    /// A header edge being dragged: column, pointer x and width at the start.
    pub column_drag: Option<(usize, f32, f32)>,
    pub preview: PreviewState,
    pub palette: Option<PaletteState>,
    pub jump: Option<JumpState>,
    pub bulk: Option<BulkState>,
    pub prompt: Option<PromptState>,
    pub confirm: Option<ConfirmState>,
    pub menu: Option<MenuState>,
    pub toolbar_menu: Option<crate::toolbar::Popup>,
    pub settings: Option<Entity<crate::settings::SettingsView>>,
    pub(crate) settings_subscription: Option<Subscription>,
    /// Settings revision the theme was last resolved against.
    theme_revision: u64,
    /// Plans proposed by background work, oldest first; the front is shown.
    pub reviews: VecDeque<Plan>,
    pub review_focus: Option<FocusHandle>,
    pub timeline: TimelineState,
    pub toasts: crate::toast::ToastState,
    /// This frame's snapshot of what waits for a paste; rows and cells test
    /// membership here instead of taking the lock each.
    pub pending: Arc<bibli_core::pending::Pending>,
    pub shortcuts: Option<crate::shortcuts::Overview>,
    /// A rubber-band selection in progress.
    pub marquee: Option<crate::elements::marquee::Drag>,
    /// The encrypted volume the passphrase prompt is for.
    pub unlock_device: Option<String>,
    pub nudges: crate::shortcuts::Nudges,
    pub last_search: String,
    pub shelf_count: usize,
    /// A file to select once the folder it lives in has listed it: a listing
    /// streams, so a link followed to another folder cannot select its target
    /// in the same frame it navigates.
    reveal: Option<Reveal>,
    /// The window's shape as last written to settings; `remember_session`
    /// compares against it so nothing is written on an ordinary frame.
    pub(crate) stored_session: crate::session::Session,
    /// Scroll position of the sidebar, so it can have a scrollbar.
    pub sidebar_scroll: gpui::ScrollHandle,
    /// A remembered view id waiting for its pane's views to be reconciled.
    restoring_view: Option<String>,
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
        crate::settings::declare(&app);
        crate::session::declare(&app);
        // Owner and group names for Details: two small files, but a read can
        // block, so not here.
        let wake = app.waker_handle();
        app.scheduler.spawn(
            bibli_core::scheduler::Priority::Background,
            "user-names",
            app.lifetime().child(),
            {
                let cache = app.cache.root().to_owned();
                move || {
                    bibli_platform::users::load();
                    // Members a killed process extracted for its previews.
                    crate::preview::sweep_members(&cache);
                    wake();
                }
            },
        );
        cx.set_global(Theme::resolve(&app, window.appearance()));
        let mut subs = Vec::new();
        subs.push(cx.observe_window_appearance(window, |this, window, cx| {
            cx.set_global(Theme::resolve(&this.app, window.appearance()));
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

        // Friendly dates say "Today" and "Yesterday": repaint once when the
        // day changes. One timer per day, not a clock.
        cx.spawn(async move |this, cx| {
            loop {
                let now = jiff::Zoned::now();
                let wait = now
                    .date()
                    .tomorrow()
                    .ok()
                    .and_then(|date| date.to_zoned(now.time_zone().clone()).ok())
                    .and_then(|midnight| now.duration_until(&midnight).try_into().ok())
                    .unwrap_or(std::time::Duration::from_secs(3600));
                cx.background_executor()
                    .timer(wait + std::time::Duration::from_secs(1))
                    .await;
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
            type_names: crate::typenames::TypeNames::default(),
            friendly_dates: true,
            sidebar_folded: std::collections::HashSet::new(),
            columns: [true; 3],
            grid_fill: false,
            zoom_wheel: crate::reader_scroll::WheelTurn::default(),
            column_widths: crate::views::list::DEFAULT_WIDTHS,
            column_drag: None,
            preview: PreviewState::default(),
            palette: None,
            jump: None,
            bulk: None,
            prompt: None,
            confirm: None,
            menu: None,
            toolbar_menu: None,
            settings: None,
            settings_subscription: None,
            theme_revision: 0,
            reviews: VecDeque::new(),
            review_focus: None,
            timeline: TimelineState::default(),
            toasts: crate::toast::ToastState::default(),
            pending: Arc::default(),
            shortcuts: None,
            marquee: None,
            unlock_device: None,
            nudges: crate::shortcuts::Nudges::default(),
            last_search: String::new(),
            shelf_count: 0,
            reveal: None,
            stored_session: crate::session::Session::default(),
            sidebar_scroll: gpui::ScrollHandle::new(),
            restoring_view: None,
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
        shell.restore_session(window, cx);
        shell
    }

    /// Put the window back the way it was left: the same view, the same
    /// split, the same panels. The view waits for `sync_views`, which is
    /// what decides which views this build and its plugins actually have.
    fn restore_session(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(session) = crate::session::restore(&self.app) else {
            return;
        };
        if session.zoom > 0 {
            self.zoom = session.zoom;
        }
        self.preview.open = session.preview;
        self.preview.details_open = session.details;
        if session.preview_width > 0. {
            self.preview.width = session.preview_width;
        }
        self.timeline.open = session.timeline;
        if !session.view.is_empty() {
            self.restoring_view = Some(session.view);
        }
        if session.split {
            self.split(window, cx);
        }
        self.stored_session = self.session();
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
        // Read the revision before the descriptors: a registration landing
        // between the two reads is then picked up by the next sync_views.
        let views_revision = self.app.registry.views.revision();
        PaneUi {
            id,
            focus: cx.focus_handle(),
            address,
            address_shown: String::new(),
            list: ListState::default(),
            view_registrations: self
                .app
                .registry
                .views
                .all()
                .into_iter()
                .map(|s| (s.id.to_string(), s.factory))
                .collect(),
            views_revision,
            views,
            view_id: view::LIST,
            grid_rows_visible: 4,
            usage: UsageState::default(),
            loading: crate::loading::State::default(),
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

    /// Show `path` in `pane`: navigate to the folder that holds it and select
    /// the file itself once the listing reaches it. A directory is simply
    /// opened. Returns false if the path is not there.
    pub fn reveal_path(
        &mut self,
        pane: PaneId,
        path: &std::path::Path,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        let Ok(metadata) = std::fs::metadata(path) else {
            return false;
        };
        let (folder, name) = if metadata.is_dir() {
            (path.to_path_buf(), None)
        } else {
            match (path.parent(), path.file_name()) {
                (Some(parent), Some(name)) => (parent.to_path_buf(), Some(name.to_owned())),
                _ => return false,
            }
        };
        self.activate_pane(pane, cx);
        self.run(
            "navigate",
            serde_json::json!({ "location": folder.to_string_lossy() }),
            window,
            cx,
        );
        self.reveal = name.map(|name| Reveal {
            pane,
            name,
            until: Instant::now() + std::time::Duration::from_secs(10),
        });
        true
    }

    /// Select what `reveal_path` is waiting for, once the listing has it.
    fn take_reveal(&mut self, cx: &mut Context<Self>) {
        let Some(reveal) = self.reveal.as_ref() else {
            return;
        };
        let found = self
            .app
            .workspace
            .read()
            .pane(reveal.pane)
            .and_then(|pane| {
                pane.listing
                    .find_by_name(&reveal.name)
                    .map(|entry| entry.id)
            });
        let Some(id) = found else {
            if reveal.until < Instant::now() {
                self.reveal = None;
            }
            return;
        };
        let pane = reveal.pane;
        self.reveal = None;
        {
            let mut ws = self.app.workspace.write();
            if let Some(pane) = ws.pane_mut(pane) {
                pane.selection.set([id]);
            }
        }
        let saved = self.pane;
        self.pane = pane;
        let index = self.cursor_index();
        if let Some(index) = index {
            self.with_view(|view, _| view.scroll_to(index));
        }
        self.pane = saved;
        cx.notify();
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
                let _ = self.app.dispatch(&Invocation::with_args(
                    "usage.release",
                    &serde_json::json!({"pane":o}),
                ));
                self.app.workspace.write().close_pane(o);
                self.panes.retain(|p| p.id != o);
            }
            window.focus(&self.ui().focus);
            self.remember_session();
            cx.notify();
            return;
        }
        let here = {
            let ws = self.app.workspace.read();
            ws.pane(self.pane).and_then(|p| p.location().cloned())
        };
        let view = self.ui().view_id;
        let id = self.app.workspace.write().add_pane();
        let mut ui = self.make_pane_ui(id, window, cx);
        // A split shows the same folder; showing it in another view as well
        // would be a second surprise.
        ui.view_id = view;
        self.panes.push(ui);
        if let Some(loc) = here
            && let Err(e) = self.app.navigate(id, &loc)
        {
            self.set_status(format!("{e:#}"), true, cx);
        }
        self.remember_session();
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

    /// The registry controls availability; panes retain only live renderers.
    pub fn available_views(&self) -> Vec<bibli_core::registry::ViewSpec> {
        let mut specs: Vec<_> = self
            .app
            .registry
            .views
            .all()
            .into_iter()
            .filter(|s| self.ui().views.contains_key(&*s.id))
            .collect();
        specs.sort_by_key(|s| s.order);
        specs
    }

    pub fn view_shortcut(&self, id: &str) -> Option<String> {
        self.key_hints
            .iter()
            .find(|h| {
                h.command == "ui.view"
                    && h.args.get("id").and_then(serde_json::Value::as_str) == Some(id)
            })
            .map(|h| h.keys.clone())
    }

    fn sync_views(&mut self) {
        // One atomic per frame: reconcile only when the registry changed since
        // a pane last looked, so descriptors are cloned per change, not per frame.
        let revision = self.app.registry.views.revision();
        if self
            .panes
            .iter()
            .all(|pane| pane.views_revision == revision)
        {
            return;
        }
        let specs = self.app.registry.views.all();
        for pane in &mut self.panes {
            if pane.views_revision == revision {
                continue;
            }
            pane.views_revision = revision;
            let replaced = view::reconcile(&mut pane.views, &mut pane.view_registrations, &specs);
            if replaced.iter().any(|id| id == "usage") {
                pane.usage = UsageState::default();
            }
            if !pane.views.contains_key(pane.view_id) {
                pane.view_id = view::LIST;
            }
        }
    }

    fn set_view(&mut self, id: &str, cx: &mut Context<Self>) {
        self.sync_views();
        let known = self.ui().views.keys().find(|k| **k == id).copied();
        match known {
            Some(k) => {
                self.ui_mut().view_id = k;
                self.remember_session();
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

    /// `ui.grid_fill`: a preference, so it goes through `settings.apply` like
    /// the switch in the dialog.
    fn toggle_grid_fill(&mut self, cx: &mut Context<Self>) {
        self.grid_fill = !self.grid_fill;
        cx.notify();
        if let Err(error) = self.app.dispatch(&Invocation::with_args(
            "settings.apply",
            &serde_json::json!({
                "changes": { crate::settings::GRID_FILL_KEY: self.grid_fill.to_string() }
            }),
        )) {
            self.set_status(format!("{error:#}"), true, cx);
        }
    }

    /// Ctrl and the wheel over the grid. The view stays where it is (the grid
    /// keeps its first visible item); only the keys follow the cursor.
    pub fn zoom_wheel(&mut self, delta: f32, lines: bool, cx: &mut Context<Self>) {
        match self.zoom_wheel.turn(delta, lines, false) {
            0 => {}
            turn => self.zoom_to(if turn < 0 { "in" } else { "out" }, cx),
        }
    }

    fn zoom_to(&mut self, dir: &str, cx: &mut Context<Self>) {
        let pos = ZOOMS.iter().position(|z| *z == self.zoom).unwrap_or(1);
        self.zoom = match dir {
            "in" => ZOOMS[(pos + 1).min(ZOOMS.len() - 1)],
            "out" => ZOOMS[pos.saturating_sub(1)],
            _ => 128,
        };
        self.remember_session();
        cx.notify();
    }

    fn zoom_step(&mut self, dir: &str, cx: &mut Context<Self>) {
        self.zoom_to(dir, cx);
        let ix = self.cursor_index();
        self.with_view(|v, _| {
            if let Some(ix) = ix {
                v.scroll_to(ix);
            }
        });
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
        if self.settings.is_some() && !name.starts_with("ui.settings") {
            return;
        }
        // The overview is something to read: a key meant for the window
        // behind it closes it instead of acting unseen.
        if self.shortcuts.is_some() && !matches!(name, "ui.shortcuts" | "ui.settings") {
            self.toggle_shortcuts(window, cx);
            return;
        }
        // Settings may contain private paths; never trace their values.
        if !name.starts_with("ui.settings") && !name.starts_with("settings.") {
            tracing::debug!(name, %args, "run");
        }
        if let Some(ui) = name.strip_prefix("ui.") {
            self.run_ui(ui, &args, window, cx);
            cx.notify();
            return;
        }
        if name == "open" && self.app.commands.get("open").is_none() {
            self.open_fallback(window, cx);
            return;
        }
        if name == "refresh" {
            if self.ui().view_id == "usage" {
                self.refresh_usage(cx);
            }
            self.reader_retry_pages();
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
            "settings" => self.open_settings(args, window, cx),
            "toolbar_next" => window.focus_next(),
            "toolbar_prev" => window.focus_prev(),
            "toolbar_menu" => self.toggle_toolbar_menu(args, window, cx),
            name if name.starts_with("settings_") => self.settings_command(name, args, window, cx),
            "shortcuts" => self.toggle_shortcuts(window, cx),
            "unlock_prompt" => {
                if let Some(device) = arg("device") {
                    let title = arg("title").unwrap_or_else(|| "this volume".into());
                    self.unlock_device = Some(device);
                    self.open_prompt(
                        crate::prompts::PromptKind::Passphrase,
                        &format!("Unlock “{title}”"),
                        "",
                        None,
                        window,
                        cx,
                    );
                }
            }
            name if name.starts_with("menu_") => self.menu_command(name, window, cx),
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
            "usage" => self.set_view("usage", cx),
            "usage_close" => self.set_view(view::LIST, cx),
            "usage_refresh" => self.refresh_usage(cx),
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
            "columns" => {
                if let Some(key) = arg("key") {
                    self.toggle_column(&key, cx);
                }
            }
            "columns_reset" => self.reset_column_widths(cx),
            "grid_fill" => self.toggle_grid_fill(cx),
            "zoom" => {
                let dir = arg("dir").unwrap_or_else(|| "reset".into());
                self.zoom_step(&dir, cx);
            }
            "inspect" => self.inspect_command(args, window, cx),
            "media_control" => self.media_control(args, cx),
            "media_scrub" => self.media_scrub(args, cx),
            "media_fullscreen" => self.media_fullscreen(window, cx),
            "media_controls" => self.media_toggle_controls(window, cx),
            "media_escape" => {
                if self.preview.media.fullscreen.is_some() {
                    self.media_exit_fullscreen(window, cx);
                } else {
                    self.preview_close(window, cx);
                }
            }
            "preview_page" => self.preview_page(
                args.get("delta")
                    .and_then(serde_json::Value::as_i64)
                    .unwrap_or(1)
                    .clamp(-1, 1) as i32,
                cx,
            ),
            "preview_seek" => self.preview_seek(
                args.get("delta")
                    .and_then(serde_json::Value::as_f64)
                    .filter(|v| v.is_finite())
                    .unwrap_or(5.0),
                cx,
            ),
            "preview_zoom" => self.preview_zoom(arg("dir").as_deref().unwrap_or("reset"), cx),
            "toggle_preview" => self.toggle_preview(window, cx),
            "preview_close" => self.preview_close(window, cx),
            "reader_continuous" => self.reader_continuous(window, cx),
            "reader_pointer" => self.reader_pointer(
                args.get("phase")
                    .and_then(serde_json::Value::as_str)
                    .unwrap_or("up"),
                args.get("page")
                    .and_then(serde_json::Value::as_u64)
                    .and_then(|p| u32::try_from(p).ok())
                    .unwrap_or(self.preview.page),
                args.get("x")
                    .and_then(serde_json::Value::as_f64)
                    .unwrap_or(0.) as f32,
                args.get("y")
                    .and_then(serde_json::Value::as_f64)
                    .unwrap_or(0.) as f32,
                window,
                cx,
            ),
            "reader_page" => self.preview_go_page(
                args.get("page")
                    .and_then(serde_json::Value::as_u64)
                    .and_then(|n| u32::try_from(n).ok())
                    .unwrap_or(1),
                cx,
            ),
            "reader_menu" | "preview_menu" => self.preview_menu(args, window, cx),
            "reader_page_edit" => self.reader_page_edit(window, cx),
            "reader_page_submit" => self.reader_page_submit(window, cx),
            "reader_page_cancel" => self.reader_page_cancel(window, cx),
            "reader_open_external" | "preview_open_external" => self.preview_open_external(cx),
            "preview_copy_path" => self.preview_copy_path(cx),
            "preview_details" => self.preview_details(cx),
            "reader_copy" => self.reader_copy(cx),
            "reader_select_all" => self.reader_select_all(cx),
            "reader_outline" => self.reader_outline(window, cx),
            "reader_find" => self.reader_find(window, cx),
            "reader_find_close" => self.reader_find_close(window, cx),
            "reader_search" => self.reader_search(cx),
            "reader_match" => self.reader_match(
                args.get("delta")
                    .and_then(serde_json::Value::as_i64)
                    .unwrap_or(1)
                    .clamp(-1, 1) as i32,
                cx,
            ),
            "reader_link" => self.reader_link(
                args.get("index")
                    .and_then(serde_json::Value::as_u64)
                    .and_then(|n| usize::try_from(n).ok())
                    .unwrap_or(usize::MAX),
                cx,
            ),
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
                crate::remember_location(&self.app);
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
    fn sync(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let t0 = Instant::now();
        let report = self.app.pump_report();
        self.sync_views();
        if let Some(id) = self.restoring_view.take()
            && self.ui().views.contains_key(id.as_str())
        {
            self.set_view(&id, cx);
            for pane in &mut self.panes {
                if let Some(known) = pane.views.keys().find(|k| ***k == *id).copied() {
                    pane.view_id = known;
                }
            }
        }
        self.app.thumbnails.tick();
        self.drain_executor(cx);
        for n in self.app.take_notifications() {
            let error = matches!(n.level, NotifyLevel::Error | NotifyLevel::Warning);
            match n.setting {
                Some(key) if error => self.set_status_about(n.text, &key, cx),
                _ => self.set_status(n.text, error, cx),
            }
        }
        self.pending = self.app.pending.get();
        let revision = self.app.settings.revision();
        if revision != self.theme_revision {
            self.theme_revision = revision;
            cx.set_global(Theme::resolve(&self.app, window.appearance()));
            self.friendly_dates = self
                .app
                .settings
                .get::<String>(crate::settings::DATES_KEY)
                .is_none_or(|format| format != "iso");
            self.grid_fill = self
                .app
                .settings
                .get::<bool>(crate::settings::GRID_FILL_KEY)
                .unwrap_or(false);
            self.columns = crate::settings::COLUMN_KEYS
                .map(|key| self.app.settings.get::<bool>(key).unwrap_or(true));
            if self.column_drag.is_none()
                && let Some(widths) = self
                    .app
                    .settings
                    .get::<Vec<f32>>(crate::settings::COLUMN_WIDTHS_KEY)
                    .and_then(|widths| <[f32; 3]>::try_from(widths).ok())
            {
                self.column_widths = widths.map(crate::views::list::clamp_width);
            }
        }
        if self.type_names.absorb() {
            cx.notify();
        }
        if let Some(settings) = self.settings.clone() {
            settings.update(cx, |settings, cx| settings.sync(window, cx));
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
        self.take_reveal(cx);
        self.sync_loading(cx);
        if report.applied > 0 || resorted_any {
            tracing::debug!(
                applied = report.applied,
                resorted = resorted_any,
                rows = self.ui().list.sorted.len(),
                took_us = t0.elapsed().as_micros() as u64,
                "sync"
            );
        }
        self.sync_preview(window, cx);
        self.sync_reader(cx);
        self.sync_document_scroll(window, cx);
        self.sync_media(window, cx);
        for id in self.panes.iter().map(|p| p.id).collect::<Vec<_>>() {
            self.with_pane(id, |shell| shell.sync_usage(cx));
        }
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
        // The Usage view draws its own scanning and empty states.
        let placeholder = (self.ui().view_id != "usage")
            .then(|| crate::placeholder::of(self, id))
            .flatten();
        let body = if let Some(placeholder) = placeholder {
            crate::placeholder::render(placeholder, cx)
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
        let message = crate::loading::message(self, id);
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
            .when_some(message, |d, message| {
                d.child(
                    div()
                        .flex_none()
                        .px_3()
                        .py_2()
                        .text_sm()
                        .text_color(theme.text_muted)
                        .bg(theme.surface)
                        .child(message),
                )
            })
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
        self.sync(window, cx);
        let theme = cx.global::<Theme>().clone();
        let toolbar = address::render_toolbar(self, cx);
        // The window's own width is known before layout, unlike an element's.
        // Below it the panes keep the room: places stay reachable through the
        // address bar, the palette and Jump.
        let side = self
            .sidebar_fits(f32::from(window.viewport_size().width))
            .then(|| sidebar::render(self, cx));
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
        let panel = preview::render_panel(self, self.preview_width_limit(window), cx);
        let status_bar = status::render(self, cx);
        // Not deferred: GPUI cannot nest deferred draws and the dialog's own
        // dropdowns are. It is the last overlay, so it paints above the rest.
        let settings = self
            .settings
            .clone()
            .map(gpui::IntoElement::into_any_element);
        let toast = crate::toast::render(self, cx);
        let shortcuts = crate::shortcuts::render(self, cx);
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
                    .children(side)
                    .children(columns)
                    .children(panel),
            )
            .child(status_bar)
            .children(toast)
            .children(overlays.into_iter().flatten())
            .children(settings)
            .children(shortcuts)
            .child(div().h(px(0.)));
        self.resize_hook(root, cx)
    }
}
