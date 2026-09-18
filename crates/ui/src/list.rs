//! Pane state shared by every view: the cached sorted order, hydration
//! bookkeeping, selection and cursor interaction, type-ahead, and the pane
//! container (focus, key context, external drops).

use std::collections::HashSet;
use std::time::{Duration, Instant};

use bibli_core::command::Invocation;
use bibli_core::workspace::PaneId;
use bibli_core::{EntryId, SortKey, SortSpec};
use gpui::{
    AnyElement, Context, ExternalPaths, KeyContext, KeyDownEvent, MouseButton, MouseDownEvent,
    Window, div, prelude::*,
};

use crate::keymap;
use crate::menu::SelectionCtx;
use crate::shell::Shell;
use crate::theme::Theme;
use crate::view::Nav;

pub struct ListState {
    /// Entry ids in display order, from the core's cached sort.
    pub sorted: std::sync::Arc<[EntryId]>,
    sorted_at: Option<(u64, SortSpec)>,
    /// Hydration requested and not yet answered; cleared when the listing changes.
    pub pending_hydrate: HashSet<EntryId>,
    pending_at: u64,
    pub visible_rows: usize,
    typeahead: String,
    typeahead_at: Option<Instant>,
}

impl Default for ListState {
    fn default() -> Self {
        ListState {
            sorted: std::sync::Arc::from([]),
            sorted_at: None,
            pending_hydrate: HashSet::new(),
            pending_at: 0,
            visible_rows: 20,
            typeahead: String::new(),
            typeahead_at: None,
        }
    }
}

impl ListState {
    /// Adopt the core's sorted order when the listing version or the spec changed.
    pub fn sync(&mut self, listing: &bibli_core::Listing, spec: SortSpec) -> bool {
        let version = listing.version();
        if self.sorted_at == Some((version, spec)) {
            return false;
        }
        self.sorted = listing.sorted(spec);
        self.sorted_at = Some((version, spec));
        if self.pending_at != version {
            self.pending_hydrate.clear();
            self.pending_at = version;
        }
        true
    }

    pub fn version(&self) -> Option<u64> {
        self.sorted_at.map(|(v, _)| v)
    }

    pub fn reset(&mut self) {
        self.sorted = std::sync::Arc::from([]);
        self.sorted_at = None;
        self.pending_hydrate.clear();
    }

    pub fn index_of(&self, id: EntryId) -> Option<usize> {
        self.sorted.iter().position(|x| *x == id)
    }
}

impl Shell {
    /// Selection summary for `when` evaluation and the pane key context.
    pub fn selection_ctx(&self) -> SelectionCtx {
        let ws = self.app.workspace.read();
        let Some(pane) = ws.pane(self.pane) else {
            return SelectionCtx::default();
        };
        let mut ctx = SelectionCtx {
            count: pane.selection.len(),
            all_dirs: true,
            all_files: true,
        };
        for id in pane.selection.iter() {
            match pane.listing.get(id) {
                Some(e) if e.is_dir() => ctx.all_files = false,
                Some(_) => ctx.all_dirs = false,
                None => {}
            }
        }
        ctx
    }

    pub fn sort_spec(&self) -> SortSpec {
        self.app
            .workspace
            .read()
            .pane(self.pane)
            .map_or_else(SortSpec::default, |p| p.sort)
    }

    /// Toggle hidden files for this pane through the core command.
    pub fn toggle_hidden(&mut self, cx: &mut Context<Self>) {
        self.dispatch_core(
            "toggle_hidden",
            serde_json::json!({ "pane": self.pane }),
            cx,
        );
    }

    /// Sort this pane by `key` through the core command (same key again flips the direction).
    pub fn sort_by(&mut self, key: SortKey, cx: &mut Context<Self>) {
        self.dispatch_core(
            "sort",
            serde_json::json!({ "key": key, "pane": self.pane }),
            cx,
        );
    }

    fn dispatch_core(&mut self, name: &str, args: serde_json::Value, cx: &mut Context<Self>) {
        let inv = bibli_core::command::Invocation {
            name: name.into(),
            args,
        };
        if let Err(e) = self.app.dispatch(&inv) {
            tracing::warn!(command = name, "dispatch failed: {e:#}");
        }
        cx.notify();
    }

    /// Ask the core for metadata on `ids` (once per listing version).
    pub fn request_hydrate(&mut self, ids: Vec<EntryId>) {
        let pane = self.pane;
        let need: Vec<EntryId> = {
            let pending = &mut self.ui_mut().list.pending_hydrate;
            ids.into_iter().filter(|id| pending.insert(*id)).collect()
        };
        if need.is_empty() {
            return;
        }
        tracing::debug!(n = need.len(), "hydrate request");
        if let Err(e) = self.app.dispatch(&Invocation::with_args(
            "hydrate",
            &serde_json::json!({ "ids": need, "pane": pane }),
        )) {
            tracing::warn!("hydrate: {e:#}");
        }
    }

    fn pane_key_context(&self) -> KeyContext {
        let mut kc = KeyContext::default();
        kc.add(keymap::PANE);
        for f in self.selection_ctx().flags() {
            kc.add(f);
        }
        if self.is_split() {
            kc.add("split");
        }
        if self.usage.active {
            kc.add("usage");
        }
        kc
    }

    pub fn select_ids(
        &mut self,
        ids: Vec<EntryId>,
        mode: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let pane = self.pane;
        self.run(
            "select",
            serde_json::json!({ "ids": ids, "mode": mode, "pane": pane }),
            window,
            cx,
        );
    }

    pub fn cursor_index(&self) -> Option<usize> {
        let ws = self.app.workspace.read();
        ws.pane(self.pane)
            .and_then(|p| p.selection.cursor)
            .and_then(|c| self.ui().list.index_of(c))
    }

    pub fn cursor_id(&self) -> Option<EntryId> {
        self.app
            .workspace
            .read()
            .pane(self.pane)
            .and_then(|p| p.selection.cursor)
    }

    /// Move the cursor to `target` (clamped), extending from the anchor if asked.
    pub fn cursor_to(
        &mut self,
        target: isize,
        extend: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let len = self.ui().list.sorted.len();
        if len == 0 {
            return;
        }
        let target = target.clamp(0, len as isize - 1) as usize;
        let id = self.ui().list.sorted[target];
        let mode = if extend { "range_to" } else { "replace" };
        self.select_ids(vec![id], mode, window, cx);
        self.with_view(|v, _| v.scroll_to(target));
        cx.notify();
    }

    pub fn cursor_move(
        &mut self,
        delta: isize,
        extend: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let cur = self.cursor_index().map_or(-1, |i| i as isize);
        let target = if cur < 0 && delta < 0 { 0 } else { cur + delta };
        self.cursor_to(target, extend, window, cx);
    }

    /// Keyboard navigation, interpreted by the active view.
    pub fn navigate_by(
        &mut self,
        nav: Nav,
        extend: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(mut view) = self.take_view() else {
            return;
        };
        let handled = view.nav(nav, self, window, cx);
        let delta = view.nav_delta(nav, self);
        self.put_view(view);
        if handled {
            return;
        }
        match (nav, delta) {
            (Nav::Home, _) => self.cursor_to(0, extend, window, cx),
            (Nav::End, _) => self.cursor_to(isize::MAX, extend, window, cx),
            (_, Some(d)) => self.cursor_move(d, extend, window, cx),
            (_, None) => {}
        }
    }

    /// Mouse down on row/cell `ix` of `pane` (which becomes active).
    pub fn on_entry_mouse_down(
        &mut self,
        pane: PaneId,
        ix: usize,
        ev: &MouseDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.activate_pane(pane, cx);
        window.focus(&self.ui().focus);
        let Some(&id) = self.ui().list.sorted.get(ix) else {
            return;
        };
        let selected = {
            let ws = self.app.workspace.read();
            ws.pane(self.pane).is_some_and(|p| p.selection.contains(id))
        };
        match ev.button {
            MouseButton::Left => {
                if ev.click_count >= 2 {
                    self.select_ids(vec![id], "replace", window, cx);
                    self.open_selection(window, cx);
                } else if ev.modifiers.control {
                    self.select_ids(vec![id], "toggle", window, cx);
                } else if ev.modifiers.shift {
                    self.select_ids(vec![id], "range_to", window, cx);
                } else if !selected {
                    // Keep a multi-selection when pressing on one of its rows,
                    // so a drag can carry all of it.
                    self.select_ids(vec![id], "replace", window, cx);
                }
            }
            MouseButton::Right => {
                if !selected {
                    self.select_ids(vec![id], "replace", window, cx);
                }
                self.open_context_menu(ev.position, cx);
            }
            _ => {}
        }
        cx.notify();
    }

    fn on_pane_key_down(&mut self, ev: &KeyDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        let ks = &ev.keystroke;
        if ks.modifiers.control
            || ks.modifiers.alt
            || ks.modifiers.platform
            || ks.modifiers.function
        {
            return;
        }
        let Some(ch) = ks.key_char.as_deref() else {
            return;
        };
        if ch.is_empty() || ch == " " || ch.chars().any(char::is_control) {
            return;
        }
        let now = Instant::now();
        {
            let l = &mut self.ui_mut().list;
            if l.typeahead_at
                .is_none_or(|t| now.duration_since(t) > Duration::from_secs(2))
            {
                l.typeahead.clear();
            }
            l.typeahead_at = Some(now);
            l.typeahead.push_str(&ch.to_lowercase());
        }
        let needle = self.ui().list.typeahead.clone();
        let found = {
            let ws = self.app.workspace.read();
            ws.pane(self.pane).and_then(|p| {
                self.ui().list.sorted.iter().position(|id| {
                    p.listing
                        .get(*id)
                        .is_some_and(|e| e.name_str().to_lowercase().starts_with(&needle))
                })
            })
        };
        if let Some(ix) = found {
            self.cursor_to(ix as isize, false, window, cx);
        }
    }

    /// Wrap a view's element in the pane container: focus, key context,
    /// type-ahead, empty-area context menu, external drops.
    pub fn pane_container(&mut self, content: AnyElement, cx: &mut Context<Self>) -> AnyElement {
        let theme = cx.global::<Theme>().clone();
        let key_context = self.pane_key_context();
        let pane = self.pane;
        div()
            .id(("pane", pane.0 as usize))
            .relative()
            .flex_1()
            .min_h_0()
            .w_full()
            .flex()
            .flex_col()
            .track_focus(&self.ui().focus)
            .key_context(key_context)
            .on_key_down(cx.listener(Shell::on_pane_key_down))
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(move |this, _, _, cx| this.activate_pane(pane, cx)),
            )
            .on_mouse_down(
                MouseButton::Right,
                cx.listener(move |this, ev: &MouseDownEvent, window, cx| {
                    this.activate_pane(pane, cx);
                    window.focus(&this.ui().focus);
                    this.open_context_menu(ev.position, cx);
                }),
            )
            .drag_over::<ExternalPaths>(move |s, _, _, _| s.bg(theme.selection_bg.opacity(0.4)))
            .on_drop(cx.listener(move |this, paths: &ExternalPaths, window, cx| {
                this.activate_pane(pane, cx);
                this.drop_external(paths.paths(), window, cx);
            }))
            .child(content)
            .into_any_element()
    }
}
