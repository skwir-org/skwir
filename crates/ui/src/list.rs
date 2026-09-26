//! Pane state shared by every view: the cached sorted order, hydration
//! bookkeeping, selection and cursor interaction, type-ahead, and the pane
//! container (focus, key context, external drops).

use std::cell::Cell;
use std::collections::HashSet;
use std::time::{Duration, Instant};

use bibli_core::command::Invocation;
use bibli_core::workspace::PaneId;
use bibli_core::{EntryId, SortKey, SortSpec};
use gpui::{
    AnyElement, Context, ExternalPaths, KeyContext, KeyDownEvent, MouseButton, MouseDownEvent,
    Window, div, prelude::*,
};

use crate::dnd::DragPayload;
use crate::keymap;
use crate::menu::SelectionCtx;
use crate::shell::Shell;
use crate::theme::Theme;
use crate::view::Nav;

pub struct ListState {
    /// Entry ids in display order, from the core's cached sort.
    pub sorted: std::sync::Arc<[EntryId]>,
    sorted_at: Option<(u64, SortSpec)>,
    /// Hydration requested and not yet answered. Kept for as long as the
    /// pane holds the same listing: an answer arrives as an update that
    /// hydrates the entry (or removes it), so nothing asked for needs asking
    /// again, and a listing that changes while 100 000 entries are in
    /// flight must not send them all a second time. A new listing, even of
    /// the folder already shown, starts bare and needs asking again.
    pub pending_hydrate: HashSet<EntryId>,
    /// The listing (`Listing::id`) everything here describes.
    listing: Option<u64>,
    pub visible_rows: usize,
    /// Index of the first entry the view laid out, and how many it did:
    /// where a tab left comes back to, and what it shows first.
    pub first_visible: usize,
    pub visible_items: usize,
    typeahead: String,
    typeahead_at: Option<Instant>,
    /// The selection summary and the (selection revision, listing version)
    /// it was computed for: read several times a frame, recomputed only
    /// when one of them moved.
    selection: Cell<Option<((u64, u64), SelectionCtx)>>,
}

impl Default for ListState {
    fn default() -> Self {
        ListState {
            sorted: std::sync::Arc::from([]),
            sorted_at: None,
            pending_hydrate: HashSet::new(),
            listing: None,
            visible_rows: 20,
            first_visible: 0,
            visible_items: 0,
            typeahead: String::new(),
            typeahead_at: None,
            selection: Cell::new(None),
        }
    }
}

impl ListState {
    /// Adopt the core's sorted order when the listing version or the spec changed.
    pub fn sync(&mut self, listing: &bibli_core::Listing, spec: SortSpec) -> bool {
        if self.listing != Some(listing.id()) {
            self.reset();
            self.listing = Some(listing.id());
        }
        let version = listing.version();
        if self.sorted_at == Some((version, spec)) {
            return false;
        }
        self.sorted = listing.sorted(spec);
        self.sorted_at = Some((version, spec));
        true
    }

    pub fn version(&self) -> Option<u64> {
        self.sorted_at.map(|(v, _)| v)
    }

    fn reset(&mut self) {
        self.sorted = std::sync::Arc::from([]);
        self.sorted_at = None;
        self.pending_hydrate.clear();
        self.selection.set(None);
    }

    /// Whether the last `sync` adopted another listing than the one before.
    pub fn listing(&self) -> Option<u64> {
        self.listing
    }

    pub fn index_of(&self, id: EntryId) -> Option<usize> {
        self.sorted.iter().position(|x| *x == id)
    }
}

/// What the selection of `pane` amounts to. Walks the whole selection.
fn summarize(pane: &bibli_core::workspace::Pane) -> SelectionCtx {
    let mut ctx = SelectionCtx {
        count: pane.selection.len(),
        all_dirs: true,
        all_files: true,
        bytes: 0,
        folder: false,
    };
    for id in pane.selection.iter() {
        match pane.listing.get(id) {
            Some(e) if e.is_dir() => ctx.all_files = false,
            Some(e) => {
                ctx.all_dirs = false;
                ctx.bytes += e
                    .metadata
                    .value(bibli_core::entry::keys::SIZE)
                    .and_then(bibli_core::Value::as_u64)
                    .unwrap_or(0);
            }
            None => {}
        }
    }
    ctx
}

impl Shell {
    /// Selection summary for `when` evaluation and the pane key context.
    /// Cached per pane on the selection's revision and the listing's
    /// version: with 100 000 entries selected, a frame must not walk them
    /// for every reader.
    pub fn selection_ctx(&self) -> SelectionCtx {
        let ws = self.app.workspace.read();
        let Some(pane) = ws.pane(self.pane) else {
            return SelectionCtx::default();
        };
        // Where the pane is, read every time: navigating replaces the
        // listing, whose version can then repeat the cached one.
        let folder = pane.location().is_some_and(bibli_core::Location::is_path);
        let key = (pane.selection.revision(), pane.listing.version());
        let cache = &self.ui().list.selection;
        if let Some((at, ctx)) = cache.get()
            && at == key
        {
            return SelectionCtx { folder, ..ctx };
        }
        let ctx = summarize(pane);
        cache.set(Some((key, ctx)));
        SelectionCtx { folder, ..ctx }
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

    /// Ask the core for metadata on `ids`, once each until the pane shows
    /// another location.
    pub fn request_hydrate(&mut self, ids: impl IntoIterator<Item = EntryId>) {
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
        if self.ui().view_id == "usage" {
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

    /// Mouse down on row/cell `ix` of `pane` (which becomes active). The
    /// middle button opens a folder in a tab behind the one shown and
    /// leaves the selection alone.
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
        self.press_collapse = None;
        let Some(&id) = self.ui().list.sorted.get(ix) else {
            return;
        };
        if ev.button == MouseButton::Middle {
            self.open_in_tab(id, window, cx);
            return;
        }
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
                    self.select_ids(vec![id], "replace", window, cx);
                } else {
                    // Keep a multi-selection when pressing on one of its rows,
                    // so a drag can carry all of it; a release without a drag
                    // makes it that one row (`on_entry_mouse_up`).
                    self.press_collapse = Some((pane, id));
                }
            }
            MouseButton::Right => {
                if !selected {
                    self.select_ids(vec![id], "replace", window, cx);
                }
                self.open_context_menu(ev.position, window, cx);
            }
            _ => {}
        }
        cx.notify();
    }

    /// Open entry `id` of the pane in a tab behind the one shown, if it is
    /// a folder.
    fn open_in_tab(&mut self, id: EntryId, window: &mut Window, cx: &mut Context<Self>) {
        let folder = {
            let ws = self.app.workspace.read();
            ws.pane(self.pane).and_then(|p| {
                p.listing.get(id).filter(|e| e.is_dir())?;
                p.open.as_ref()?.path_of(id)
            })
        };
        if let Some(folder) = folder {
            let pane = self.pane;
            self.run(
                "tab.new",
                serde_json::json!({
                    "location": bibli_core::os_path::PathArg::from(folder),
                    "show": false,
                    "pane": pane,
                }),
                window,
                cx,
            );
        }
    }

    /// The release of a plain click on a row of a multi-selection: the
    /// selection becomes that row, as in every file manager. A drag that
    /// started from the press has already cleared the request.
    pub fn on_entry_mouse_up(
        &mut self,
        pane: PaneId,
        ix: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some((pressed, id)) = self.press_collapse.take() else {
            return;
        };
        let here = self.with_pane(pane, |shell| shell.ui().list.sorted.get(ix).copied());
        if pressed == pane && here == Some(id) {
            self.with_pane(pane, |shell| {
                shell.select_ids(vec![id], "replace", window, cx);
            });
        }
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

    // ------------------------------------------------------------ marquee

    /// A press on the empty part of a view: the selection goes (unless Ctrl
    /// or Shift keeps it) and a rubber band may follow.
    pub fn marquee_begin(
        &mut self,
        pane: PaneId,
        start: (f32, f32),
        position: gpui::Point<gpui::Pixels>,
        keep: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.activate_pane(pane, cx);
        window.focus(&self.ui().focus);
        let kept = if keep {
            let ws = self.app.workspace.read();
            ws.pane(pane)
                .map(|p| p.selection.iter().collect())
                .unwrap_or_default()
        } else {
            self.select_ids(Vec::new(), "replace", window, cx);
            Vec::new()
        };
        self.marquee = Some(crate::elements::marquee::Drag {
            pane,
            start,
            current: position,
            covered: Vec::new(),
            kept,
        });
        cx.notify();
    }

    pub fn marquee_move(
        &mut self,
        position: gpui::Point<gpui::Pixels>,
        covered: Vec<usize>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(drag) = self.marquee.as_mut() else {
            return;
        };
        drag.current = position;
        if drag.covered != covered {
            drag.covered.clone_from(&covered);
            let mut ids = drag.kept.clone();
            let sorted = &self.ui().list.sorted;
            ids.extend(
                covered
                    .iter()
                    .filter_map(|index| sorted.get(*index).copied()),
            );
            self.select_ids(ids, "replace", window, cx);
        }
        cx.notify();
    }

    pub fn marquee_end(&mut self, cx: &mut Context<Self>) {
        if self.marquee.take().is_some() {
            cx.notify();
        }
    }

    /// Wrap a view's element in the pane container: focus, key context,
    /// type-ahead, empty-area context menu, external drops.
    pub fn pane_container(&mut self, content: AnyElement, cx: &mut Context<Self>) -> AnyElement {
        let tint = cx.global::<Theme>().drop_target;
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
                    // A row already opened its menu for this click. On the
                    // background the menu is the folder's: drop the selection
                    // so the menu, and the commands it runs, are about the
                    // folder and not about files elsewhere in the list.
                    let on_row = this
                        .menu
                        .as_ref()
                        .is_some_and(|m| m.position == ev.position);
                    if !on_row {
                        this.run(
                            "select_none",
                            serde_json::json!({ "pane": pane }),
                            window,
                            cx,
                        );
                    }
                    this.open_context_menu(ev.position, window, cx);
                }),
            )
            // A drop beside the rows lands in the folder the pane shows,
            // from the other pane or from another application.
            .drag_over::<DragPayload>(move |s, payload: &DragPayload, _, _| {
                // Its own rows are already in this folder.
                if payload.pane == pane { s } else { s.bg(tint) }
            })
            .on_drop(cx.listener(move |this, payload: &DragPayload, window, cx| {
                if payload.pane == pane {
                    return;
                }
                match this.pane_folder(pane) {
                    Some(dest) => this.drop_into(payload.pane, &dest, window, cx),
                    None => this.set_status("drop target is not a directory", true, cx),
                }
            }))
            .drag_over::<ExternalPaths>(move |s, _, _, _| s.bg(tint))
            .on_drop(cx.listener(move |this, paths: &ExternalPaths, window, cx| {
                this.activate_pane(pane, cx);
                this.drop_external(paths.paths(), window, cx);
            }))
            .child(content)
            .into_any_element()
    }
}

/// The pointer on a row or cell `ix` of `pane`: press to select (right
/// button: and open the menu), release to collapse a multi-selection. One
/// definition for every view, so they cannot drift apart.
pub fn entry_mouse<E: InteractiveElement>(
    el: E,
    pane: PaneId,
    ix: usize,
    cx: &mut Context<Shell>,
) -> E {
    let press = move |this: &mut Shell,
                      ev: &MouseDownEvent,
                      window: &mut Window,
                      cx: &mut Context<Shell>| {
        this.on_entry_mouse_down(pane, ix, ev, window, cx);
    };
    el.on_mouse_down(MouseButton::Left, cx.listener(press))
        .on_mouse_down(MouseButton::Right, cx.listener(press))
        .on_mouse_down(MouseButton::Middle, cx.listener(press))
        .on_mouse_up(
            MouseButton::Left,
            cx.listener(move |this, _: &gpui::MouseUpEvent, window, cx| {
                this.on_entry_mouse_up(pane, ix, window, cx);
            }),
        )
}
