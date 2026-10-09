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

/// What a keyboard move does to the selection: select the item reached
/// (arrows), extend to it from the anchor (Shift), or only move the cursor
/// (Ctrl), so Ctrl+Space can then pick items that are not side by side.
/// A key scheme's moves follow (`SelectMode::Follow`, ADR-024): the
/// selection moves with the cursor unless it was picked.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Step {
    Select,
    Extend,
    Cursor,
    Follow,
}

impl Step {
    /// `ui.cursor`'s `step` argument.
    fn parse(step: Option<&str>) -> Step {
        match step {
            Some("extend") => Step::Extend,
            Some("move") => Step::Cursor,
            Some("follow") => Step::Follow,
            _ => Step::Select,
        }
    }
}

/// `ui.cursor`'s `to` argument; `here` is the item under the cursor.
fn nav_of(to: &str) -> Option<Nav> {
    Some(match to {
        "up" => Nav::Up,
        "down" => Nav::Down,
        "left" => Nav::Left,
        "right" => Nav::Right,
        "page_up" => Nav::PageUp,
        "page_down" => Nav::PageDown,
        "half_page_up" => Nav::HalfPageUp,
        "half_page_down" => Nav::HalfPageDown,
        "home" => Nav::Home,
        "end" => Nav::End,
        _ => return None,
    })
}

/// Whether `name` matches what `/` looks for: anywhere in the name, case
/// ignored unless the pattern has a capital (yazi's smart case).
pub fn find_matches(name: &str, pattern: &str) -> bool {
    if pattern.chars().any(char::is_uppercase) {
        name.contains(pattern)
    } else {
        name.to_lowercase().contains(&pattern.to_lowercase())
    }
}

/// How long a pause ends a name being typed.
const TYPING_PAUSE: Duration = Duration::from_secs(2);

/// A name typed to reach an entry (type-ahead). It belongs to the listing
/// it was typed in: once the pane shows another (a folder opened with
/// Enter, Back), the next character begins a new name, as after a pause.
struct Typed {
    /// What was typed, lowercase.
    text: String,
    /// The listing (`Listing::id`) it was typed in.
    listing: u64,
    /// When its last character was typed.
    at: Instant,
    /// The selection's revision while the listing may still bring an entry
    /// the name reaches first (none yet, or one sorted before the one
    /// selected): each order it brings is looked through again
    /// (`Shell::seek_typed_again`), unless the selection changed since, as
    /// a selection made meanwhile wins.
    waiting: Option<u64>,
}

impl Typed {
    fn new(listing: u64, now: Instant) -> Self {
        Typed {
            text: String::new(),
            listing,
            at: now,
            waiting: None,
        }
    }

    /// Whether a character typed in `listing` at `now` goes on with this
    /// name.
    fn goes_on(&self, listing: u64, now: Instant) -> bool {
        self.listing == listing && now.duration_since(self.at) <= TYPING_PAUSE
    }

    fn push(&mut self, ch: &str, now: Instant) {
        self.text.extend(ch.chars().flat_map(char::to_lowercase));
        self.at = now;
    }

    /// Whether `name` begins with what was typed, case ignored.
    fn begins(&self, name: &str) -> bool {
        let mut name = name.chars().flat_map(char::to_lowercase);
        self.text.chars().all(|c| name.next() == Some(c))
    }
}

pub struct ListState {
    /// Entry ids in display order, from the core's cached sort.
    pub sorted: std::sync::Arc<[EntryId]>,
    sorted_at: Option<(u64, u64, SortSpec, u64)>,
    /// Hydration requested and not yet answered. Kept for as long as the
    /// pane holds the same listing: an answer arrives as an update that
    /// hydrates the entry (or removes it), so nothing asked for needs asking
    /// again, and a listing that changes while 100 000 entries are in
    /// flight must not send them all a second time. A new listing, even of
    /// the folder already shown, starts bare and needs asking again.
    /// Each by the listing that holds it (`Pane::listing_of`): an entry
    /// that moves to another (between two folders opened in place) is
    /// asked for again there.
    pub pending_hydrate: HashSet<(u64, EntryId)>,
    /// The listing (`Listing::id`) everything here describes.
    listing: Option<u64>,
    /// The listing's `removed_version` when `pending_hydrate` last let go
    /// of the entries taken out.
    removed: u64,
    pub visible_rows: usize,
    /// How many entries the view laid out: what a tab shown again shows
    /// first.
    pub visible_items: usize,
    typed: Option<Typed>,
    /// The selection summary and the (selection revision, listing version)
    /// it was computed for: read several times a frame, recomputed only
    /// when one of them moved.
    selection: Cell<Option<((u64, u64, u64), SelectionCtx)>>,
}

impl Default for ListState {
    fn default() -> Self {
        ListState {
            sorted: std::sync::Arc::from([]),
            sorted_at: None,
            pending_hydrate: HashSet::new(),
            listing: None,
            removed: 0,
            visible_rows: 20,
            visible_items: 0,
            typed: None,
            selection: Cell::new(None),
        }
    }
}

impl ListState {
    /// Adopt the pane's order (`Pane::order`: the listing sorted, and the
    /// folders opened in place) when the listing's version, the opened
    /// folders or the spec changed.
    pub fn sync(&mut self, pane: &bibli_core::workspace::Pane) -> bool {
        let listing = &pane.listing;
        if self.listing != Some(listing.id()) {
            self.reset();
            self.listing = Some(listing.id());
            // A name typed in the listing before has ended; one typed in
            // this one before the sync adopted it (a key right after Enter)
            // goes on.
            self.typed.take_if(|typed| typed.listing != listing.id());
        }
        // An entry taken out and back (a file untagged, then given its tag
        // again by Undo) comes back bare: it is asked for again.
        if self.removed != listing.removed_version() {
            self.removed = listing.removed_version();
            self.pending_hydrate
                .retain(|(listing, id)| pane.listing_of(*id) == Some(*listing));
        }
        let at = (
            listing.version(),
            pane.tree_version(),
            pane.sort,
            pane.filter_version(),
        );
        if self.sorted_at == Some(at) {
            return false;
        }
        self.sorted = pane.order();
        self.sorted_at = Some(at);
        true
    }

    pub fn version(&self) -> Option<u64> {
        self.sorted_at.map(|(v, _, _, _)| v)
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
        all_places: true,
        ..SelectionCtx::default()
    };
    for id in pane.selection.iter() {
        ctx.all_places &= id.is_place();
        match pane.entry(id) {
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
        let verbs = &self.ui().place.verbs;
        let here = |ctx: SelectionCtx| SelectionCtx {
            folder,
            refs: verbs.remove.is_some(),
            drops: verbs.drop.is_some(),
            creates: verbs.create.is_some(),
            restores: verbs.restore.is_some(),
            ..ctx
        };
        let key = (
            pane.selection.revision(),
            pane.listing.version(),
            pane.tree_version(),
        );
        let cache = &self.ui().list.selection;
        if let Some((at, ctx)) = cache.get()
            && at == key
        {
            return here(ctx);
        }
        let ctx = summarize(pane);
        cache.set(Some((key, ctx)));
        here(ctx)
    }

    /// What the selected entries are, by name (a link's target's), for a
    /// menu's `kind:` and `has:` tokens. Walks the selection: when a menu
    /// opens, never per frame.
    pub fn selection_types(&self) -> crate::menu::SelectionTypes {
        let entries: Vec<(std::ffi::OsString, Option<&'static str>)> = {
            let ws = self.app.workspace.read();
            let Some(pane) = ws.pane(self.pane) else {
                return crate::menu::SelectionTypes::default();
            };
            pane.selection
                .iter()
                .filter_map(|id| pane.entry(id))
                .map(|e| (e.type_name().to_owned(), e.inode_type()))
                .collect()
        };
        crate::menu::SelectionTypes::of(
            entries
                .iter()
                .map(|(name, inode)| (name.as_os_str(), *inode)),
        )
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
        self.sort(Some(key), None, cx);
    }

    /// The core `sort` command for this pane: a key, a direction, or both.
    /// A key without a direction flips it when it is the current key.
    pub fn sort(&mut self, key: Option<SortKey>, ascending: Option<bool>, cx: &mut Context<Self>) {
        self.dispatch_core(
            "sort",
            serde_json::json!({ "key": key, "ascending": ascending, "pane": self.pane }),
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
        // Only what lacks its metadata, of whichever listing of the pane
        // holds it (shown, or opened in place): callers pass what they draw.
        let held: Vec<(u64, EntryId)> = {
            let ws = self.app.workspace.read();
            let Some(p) = ws.pane(pane) else { return };
            ids.into_iter()
                .filter(|id| p.entry(*id).is_some_and(|e| !e.metadata.is_hydrated()))
                .filter_map(|id| Some((p.listing_of(id)?, id)))
                .collect()
        };
        let need: Vec<EntryId> = {
            let pending = &mut self.ui_mut().list.pending_hydrate;
            held.into_iter()
                .filter(|key| pending.insert(*key))
                .map(|(_, id)| id)
                .collect()
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
        // The view shown, by its id: a lens's keys (Usage's) say it.
        kc.add(self.ui().view_id);
        let (sweeping, filtered) = {
            let ws = self.app.workspace.read();
            ws.pane(self.pane).map_or((false, false), |p| {
                (p.selection.sweeping().is_some(), p.filter().is_some())
            })
        };
        // Escape shows everything again before it deselects.
        if filtered {
            kc.add("filtered");
        }
        if sweeping {
            kc.add("sweep");
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

    /// Move the cursor to `target` (clamped): selecting it, extending from
    /// the anchor, or leaving the selection alone, as `step` says.
    pub fn cursor_to(
        &mut self,
        target: isize,
        step: Step,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let len = self.ui().list.sorted.len();
        if len == 0 {
            return;
        }
        let target = target.clamp(0, len as isize - 1) as usize;
        let id = self.ui().list.sorted[target];
        let mode = match step {
            Step::Select => "replace",
            Step::Extend => "range_to",
            Step::Cursor => "cursor",
            Step::Follow => "follow",
        };
        self.select_ids(vec![id], mode, window, cx);
        self.with_view(|v, _| v.scroll_to(target));
        cx.notify();
    }

    pub fn cursor_move(
        &mut self,
        delta: isize,
        step: Step,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let cur = self.cursor_index().map_or(-1, |i| i as isize);
        let target = if cur < 0 && delta < 0 { 0 } else { cur + delta };
        self.cursor_to(target, step, window, cx);
    }

    /// Ctrl+Space: select or deselect the item under the cursor.
    pub fn toggle_cursor(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(id) = self.cursor_id() else {
            return;
        };
        self.select_ids(vec![id], "toggle", window, cx);
    }

    /// `ui.cursor_pick`: pick or unpick the item under the cursor (the
    /// first, before the cursor has one), then move to the next as yazi's
    /// Space does unless `advance` is false.
    pub fn pick_cursor(&mut self, advance: bool, window: &mut Window, cx: &mut Context<Self>) {
        let Some(id) = self
            .cursor_id()
            .or_else(|| self.ui().list.sorted.first().copied())
        else {
            return;
        };
        self.select_ids(vec![id], "pick", window, cx);
        if advance {
            self.cursor_move(1, Step::Follow, window, cx);
        }
    }

    /// `ui.cursor { to, step }`: every keyboard move with every step, so a
    /// key scheme binds moves without a command of its own for each.
    pub fn cursor_command(
        &mut self,
        args: &serde_json::Value,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let text = |key| args.get(key).and_then(serde_json::Value::as_str);
        let step = Step::parse(text("step"));
        match text("to").unwrap_or("down") {
            "here" => {
                if let Some(ix) = self.cursor_index() {
                    self.cursor_to(ix as isize, step, window, cx);
                }
            }
            // One item along the pane's order, whatever the view draws:
            // how peek walks the files.
            "previous" => self.cursor_move(-1, step, window, cx),
            "next" => self.cursor_move(1, step, window, cx),
            to => match nav_of(to) {
                Some(nav) => self.navigate_by(nav, step, window, cx),
                None => self.set_status(format!("ui.cursor cannot go to “{to}”"), true, cx),
            },
        }
    }

    /// The next entry after `from` (the cursor when `None`), or before it
    /// when `backward`, whose name `find_matches` the pattern, around the
    /// end.
    pub fn find_from(&self, from: Option<EntryId>, pattern: &str, backward: bool) -> Option<usize> {
        if pattern.is_empty() {
            return None;
        }
        let ws = self.app.workspace.read();
        let pane = ws.pane(self.pane)?;
        let sorted = &self.ui().list.sorted;
        let len = sorted.len();
        let start = from
            .or(pane.selection.cursor)
            .and_then(|c| sorted.iter().position(|id| *id == c));
        (1..=len)
            .map(|step| match (start, backward) {
                (Some(s), false) => (s + step) % len,
                (Some(s), true) => (s + len - step % len) % len,
                (None, false) => step - 1,
                (None, true) => len - step,
            })
            .find(|ix| {
                pane.entry(sorted[*ix])
                    .is_some_and(|e| find_matches(&e.name_str(), pattern))
            })
    }

    /// Keyboard navigation, interpreted by the active view.
    pub fn navigate_by(
        &mut self,
        nav: Nav,
        step: Step,
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
        // A view that says where Home and End go (the board: its column's
        // ends) is followed; else they are the order's.
        match (nav, delta) {
            (_, Some(d)) => self.cursor_move(d, step, window, cx),
            (Nav::Home, None) => self.cursor_to(0, step, window, cx),
            (Nav::End, None) => self.cursor_to(isize::MAX, step, window, cx),
            (_, None) => {}
        }
    }

    /// Mouse down on entry `id` of `pane` (which becomes active). The
    /// middle button opens a folder in a tab behind the one shown and
    /// leaves the selection alone.
    pub fn on_entry_mouse_down(
        &mut self,
        pane: PaneId,
        id: EntryId,
        ev: &MouseDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.activate_pane(pane, cx);
        window.focus(&self.ui().focus);
        self.press_collapse = None;
        // Gone since it was drawn: the press is on nothing.
        if self.ui().list.index_of(id).is_none() {
            return;
        }
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
                } else if ev.modifiers.secondary() && ev.modifiers.shift {
                    // A range added to what is selected, not in its place.
                    self.select_ids(vec![id], "add_range", window, cx);
                } else if ev.modifiers.secondary() {
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

    /// Ctrl+Enter: the selected folders and places each in a tab behind
    /// the one shown, as the middle button opens one (`tab.open`); with
    /// none selected, the selection opens as Enter would.
    pub fn open_selection_in_tabs(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let leads = {
            let ws = self.app.workspace.read();
            ws.pane(self.pane)
                .is_some_and(|p| p.selection.iter().any(|id| p.leads_to(id).is_some()))
        };
        if !leads {
            return self.open_selection(window, cx);
        }
        let pane = self.pane;
        self.run("tab.open", serde_json::json!({ "pane": pane }), window, cx);
    }

    /// Open entry `id` of the pane in a tab behind the one shown, if it
    /// leads somewhere: a folder, a place, a trashed folder.
    fn open_in_tab(&mut self, id: EntryId, window: &mut Window, cx: &mut Context<Self>) {
        let leads = {
            let ws = self.app.workspace.read();
            ws.pane(self.pane).is_some_and(|p| p.leads_to(id).is_some())
        };
        if leads {
            let pane = self.pane;
            self.run(
                "tab.open",
                serde_json::json!({ "ids": [id], "pane": pane }),
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
        released: EntryId,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some((pressed, id)) = self.press_collapse.take() else {
            return;
        };
        if pressed == pane && released == id {
            self.with_pane(pane, |shell| {
                shell.select_ids(vec![id], "replace", window, cx);
            });
        }
    }

    fn on_pane_key_down(&mut self, ev: &KeyDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        // Letters are commands while the keymap takes them here.
        if !self.keys.typeahead {
            return;
        }
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
        // The listing shown now: one a key just opened, even before the
        // next sync adopts it.
        let Some(listing) = self
            .app
            .workspace
            .read()
            .pane(self.pane)
            .map(|p| p.listing.id())
        else {
            return;
        };
        {
            let l = &mut self.ui_mut().list;
            l.typed.take_if(|typed| !typed.goes_on(listing, now));
            // A path begins: `/` or `~` typed first opens the address bar
            // with it, as in GNOME's file chooser and Nautilus.
            if l.typed.is_none() && matches!(ch, "/" | "~") {
                let (address, text) = (self.ui().address.clone(), ch.to_owned());
                address.update(cx, |input, cx| {
                    input.set_text(text, cx);
                    input.focus(window);
                });
                cx.notify();
                return;
            }
            l.typed
                .get_or_insert_with(|| Typed::new(listing, now))
                .push(ch, now);
        }
        let Some((found, whole)) = self.seek_typed() else {
            return;
        };
        if let Some(ix) = found {
            self.cursor_to(ix as isize, Step::Select, window, cx);
        }
        self.typed_waits(whole);
    }

    /// Where the name typed leads: the first entry in the order shown whose
    /// name begins with it, and whether that order is the whole listing it
    /// was typed in. A folder just opened is adopted at the next sync, and
    /// streams: until it is whole, an entry the name reaches first may come.
    fn seek_typed(&self) -> Option<(Option<usize>, bool)> {
        let ws = self.app.workspace.read();
        let pane = ws.pane(self.pane)?;
        let list = &self.ui().list;
        let typed = list.typed.as_ref()?;
        if pane.listing.id() != typed.listing || list.listing != Some(typed.listing) {
            return Some((None, false));
        }
        let found = list
            .sorted
            .iter()
            .position(|id| pane.entry(*id).is_some_and(|e| typed.begins(&e.name_str())));
        let whole = list.version() == Some(pane.listing.version())
            && (pane.listing.is_complete() || pane.listing.error().is_some());
        Some((found, whole))
    }

    /// Once the name typed has selected what it reaches so far: keep looking
    /// as the listing brings more, unless it is whole.
    fn typed_waits(&mut self, whole: bool) {
        let revision = if whole {
            None
        } else {
            let ws = self.app.workspace.read();
            ws.pane(self.pane).map(|p| p.selection.revision())
        };
        if let Some(typed) = self.ui_mut().list.typed.as_mut() {
            typed.waiting = revision;
        }
    }

    /// Look again for a name typed while its listing was not whole
    /// (`seek_typed`): it brought more. Called by the sync, once it adopted
    /// them.
    pub(crate) fn seek_typed_again(&mut self, cx: &mut Context<Self>) {
        let Some(waiting) = self.ui().list.typed.as_ref().and_then(|t| t.waiting) else {
            return;
        };
        let (revision, cursor) = {
            let ws = self.app.workspace.read();
            ws.pane(self.pane)
                .map(|p| (p.selection.revision(), p.selection.cursor))
                .unzip()
        };
        // A selection made meanwhile wins.
        if revision != Some(waiting) {
            if let Some(typed) = self.ui_mut().list.typed.as_mut() {
                typed.waiting = None;
            }
            return;
        }
        let Some((found, whole)) = self.seek_typed() else {
            return;
        };
        if let Some(ix) = found {
            let id = self.ui().list.sorted[ix];
            if cursor.flatten() != Some(id)
                && let Some(pane) = self.app.workspace.write().pane_mut(self.pane)
            {
                pane.selection.set([id]);
            }
            // Again when it was already selected: entries that arrived
            // before it moved it down.
            self.with_view(|view, _| view.scroll_to(ix));
            cx.notify();
        }
        self.typed_waits(whole);
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
            .children(self.menu_area_marker(pane))
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
                this.give_to_pane(payload.pane, pane, None, window, cx);
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

/// The pointer on entry `id`'s row or cell of `pane`: press to select
/// (right button: and open the menu), release to collapse a
/// multi-selection. One definition for every view, so they cannot drift
/// apart.
pub fn entry_mouse<E: InteractiveElement>(
    el: E,
    pane: PaneId,
    id: EntryId,
    cx: &mut Context<Shell>,
) -> E {
    let press = move |this: &mut Shell,
                      ev: &MouseDownEvent,
                      window: &mut Window,
                      cx: &mut Context<Shell>| {
        this.on_entry_mouse_down(pane, id, ev, window, cx);
    };
    el.on_mouse_down(MouseButton::Left, cx.listener(press))
        .on_mouse_down(MouseButton::Right, cx.listener(press))
        .on_mouse_down(MouseButton::Middle, cx.listener(press))
        .on_mouse_up(
            MouseButton::Left,
            cx.listener(move |this, _: &gpui::MouseUpEvent, window, cx| {
                this.on_entry_mouse_up(pane, id, window, cx);
            }),
        )
}

#[cfg(test)]
mod tests {
    use std::time::{Duration, Instant};

    use super::{TYPING_PAUSE, Typed, find_matches};

    #[test]
    fn a_find_ignores_case_until_the_pattern_has_a_capital() {
        assert!(find_matches("Rapport d'été.pdf", "été"));
        assert!(find_matches("Rapport d'été.pdf", "rapp"));
        assert!(!find_matches("rapport.pdf", "Rapp"));
        assert!(find_matches("Rapport.pdf", "Rapp"));
    }

    /// Typed in the folder Enter left, a name does not go on in the one it
    /// opened however quickly the next key follows.
    #[test]
    fn a_name_typed_ends_with_its_listing_or_a_pause() {
        let start = Instant::now();
        let mut typed = Typed::new(1, start);
        typed.push("d", start);
        typed.push("o", start);
        let soon = start + Duration::from_millis(300);
        assert!(typed.goes_on(1, soon));
        assert!(!typed.goes_on(2, soon));
        assert!(!typed.goes_on(1, start + TYPING_PAUSE + Duration::from_millis(1)));
    }

    #[test]
    fn a_name_typed_begins_names_whatever_their_case() {
        let now = Instant::now();
        let mut typed = Typed::new(1, now);
        for ch in ["P", "i", "C"] {
            typed.push(ch, now);
        }
        assert!(typed.begins("pictures"));
        assert!(typed.begins("PICTURES"));
        assert!(!typed.begins("pi"));
        assert!(!typed.begins("papers"));
        let mut accented = Typed::new(1, now);
        accented.push("É", now);
        assert!(accented.begins("été 2026"));
        assert!(Typed::new(1, now).begins("anything"));
    }
}
