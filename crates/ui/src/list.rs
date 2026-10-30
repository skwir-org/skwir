//! Pane state shared by every view: the cached sorted order, hydration
//! bookkeeping, selection and cursor interaction, type-ahead, and the pane
//! container (focus, key context, external drops).

use std::cell::Cell;
use std::collections::HashMap;
use std::time::{Duration, Instant};

use gpui::{
    AnyElement, Context, ExternalPaths, KeyContext, KeyDownEvent, MouseButton, MouseDownEvent,
    Window, div, prelude::*, px,
};
use skwir_core::app::{HydrateArgs, SelectArgs, SelectMode};
use skwir_core::workspace::PaneId;
use skwir_core::{EntryId, SortKey, SortSpec};

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

    /// How the core's `select` takes a move with this step to an entry,
    /// and whether it is told the row moved to (`SelectArgs::at`): a range
    /// and a following move look for the entry in the order otherwise.
    fn select(self) -> (SelectMode, bool) {
        match self {
            Step::Select => (SelectMode::Replace, false),
            Step::Extend => (SelectMode::RangeTo, true),
            Step::Cursor => (SelectMode::Cursor, false),
            Step::Follow => (SelectMode::Follow, true),
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
    skwir_core::workspace::NameFilter::new(pattern).holds(std::ffi::OsStr::new(name))
}

/// How long a pause ends a name being typed.
const TYPING_PAUSE: Duration = Duration::from_secs(2);

/// A name typed to reach an entry (type-ahead). It belongs to the listing
/// it was typed in: once the pane shows another (a folder opened with
/// Enter, Back), the next character begins a new name, as after a pause.
struct Typed {
    /// What was typed, as typed.
    text: String,
    /// The listing (`Listing::id`) it was typed in.
    listing: u64,
    /// When its last character was typed.
    at: Instant,
    /// Where the look begins: the entry under the cursor when the last
    /// character was typed (none: the order's first), and whether it is
    /// passed over. A new name and a letter typed again go on past it, so
    /// each press reaches the next name; a name typed on stays on the
    /// entry while its name still begins so. Round the end after the last.
    from: Option<EntryId>,
    past: bool,
    /// Whether the listing, whole, has no name beginning so: the label
    /// says it.
    missed: bool,
    /// The selection's revision while the listing may still bring an entry
    /// the name reaches first (none yet, or one sorted before the one
    /// selected): each order it brings is looked through again
    /// (`Shell::seek_typed_again`), unless the selection changed since, as
    /// a selection made meanwhile wins.
    waiting: Option<u64>,
    /// The listing's names when the order was last looked through while it
    /// waits: sorted by what names say, an order brings nothing new until
    /// names come, so it is not looked through at every slice streamed.
    looked: Option<u64>,
}

impl Typed {
    fn new(listing: u64, now: Instant) -> Self {
        Typed {
            text: String::new(),
            listing,
            at: now,
            from: None,
            past: false,
            missed: false,
            waiting: None,
            looked: None,
        }
    }

    /// Whether a character typed in `listing` at `now` goes on with this
    /// name.
    fn goes_on(&self, listing: u64, now: Instant) -> bool {
        self.listing == listing && now.duration_since(self.at) <= TYPING_PAUSE
    }

    fn push(&mut self, ch: &str, now: Instant) {
        self.text.push_str(ch);
        self.at = now;
    }

    /// Whether one letter was typed again and again ("ddd", case
    /// ignored): each press then reaches the next name beginning with it,
    /// as in Dolphin and Explorer, never a name beginning "ddd".
    fn repeats(&self) -> bool {
        let mut letters = self.text.chars().flat_map(char::to_lowercase);
        let Some(first) = letters.next() else {
            return false;
        };
        let mut again = false;
        for letter in letters {
            if letter != first {
                return false;
            }
            again = true;
        }
        again
    }

    /// What a name must begin with: what was typed, or its letter once
    /// when that one letter was typed again and again.
    fn sought(&self) -> &str {
        match self.text.chars().next() {
            Some(first) if self.repeats() => &self.text[..first.len_utf8()],
            _ => &self.text,
        }
    }

    /// `sought`, folded once as the order folds names (`collate::Prefix`):
    /// "ecole" reaches "École", which sorts beside it.
    fn prefix(&self) -> skwir_core::collate::Prefix {
        skwir_core::collate::Prefix::new(self.sought())
    }
}

/// The first item of `order` that `holds`, going on from `from` (past it
/// when `past`) and round the end. One walk, with no index of the order
/// made: a folder streaming changes its order at every slice, and a name
/// typed is looked for again at each.
fn seek_from<T: Copy + PartialEq>(
    order: &[T],
    from: Option<T>,
    past: bool,
    holds: impl Fn(T) -> bool,
) -> Option<usize> {
    let mut passed = from.is_none();
    // What holds before `from`: reached round the end.
    let mut first = None;
    for (ix, &item) in order.iter().enumerate() {
        let here = !passed && Some(item) == from;
        passed |= here;
        // Before `from`, once one holds, only `from` is looked for.
        if !passed && first.is_some() {
            continue;
        }
        if !holds(item) {
            continue;
        }
        if passed && !(here && past) {
            return Some(ix);
        }
        first.get_or_insert(ix);
    }
    first
}

pub struct ListState {
    /// Entry ids in display order, from the core's cached sort.
    pub sorted: std::sync::Arc<[EntryId]>,
    sorted_at: Option<(u64, u64, SortSpec, u64)>,
    /// Where each of `sorted` is, made the first time it is asked for after
    /// the order changed (`index_of`): a card or a cursor found its place by
    /// walking the whole order.
    positions: std::cell::OnceCell<HashMap<EntryId, usize>>,
    /// Hydration requested and not yet answered. Kept for as long as the
    /// pane holds the same listing: an answer arrives as an update that
    /// hydrates the entry (or removes it), so nothing asked for needs asking
    /// again, and a listing that changes while 100 000 entries are in
    /// flight must not send them all a second time. A new listing, even of
    /// the folder already shown, starts bare and needs asking again.
    /// Each by the listing that holds it (`Pane::listing_of`): an entry
    /// that moves to another (between two folders opened in place) is
    /// asked for again there. With whether it was asked for on screen: one
    /// asked for off screen (a sort by size reads them all) and drawn
    /// since is asked for again, first.
    pub pending_hydrate: HashMap<(u64, EntryId), bool>,
    /// The listing (`Listing::id`) everything here describes.
    listing: Option<u64>,
    /// The listing's `removed_version` when `pending_hydrate` last let go
    /// of the entries taken out.
    removed: u64,
    /// What `sync_sort_hydration` last asked the metadata of the whole
    /// pane for: the listing, its names, the folders opened, the sort and
    /// the filter.
    sort_hydrated: Option<(u64, u64, u64, SortSpec, u64)>,
    /// What `sync_selection_hydration` last asked for: the listing and the
    /// selection's revision.
    selection_hydrated: Option<(u64, u64)>,
    pub visible_rows: usize,
    /// How many entries the view laid out: what a tab shown again shows
    /// first.
    pub visible_items: usize,
    typed: Option<Typed>,
    /// The redraw that takes the name typed's label away once the pause
    /// has ended: one, not a loop; the next character replaces it.
    typed_label: Option<gpui::Task<()>>,
    /// The selection summary and the (selection revision, listing names,
    /// opened folders) it was computed for: read several times a frame,
    /// recomputed only when one of them moved.
    selection: Cell<Option<((u64, u64, u64), SelectionCtx)>>,
    /// What the selection weighs, and the sizes it was added up for.
    selection_bytes: Cell<Option<SelectionBytes>>,
}

/// What the selected files weigh, as last asked: for which summary and
/// which sizes of the listing, and while a file's size is not known,
/// where in the selection one was found, the next look's start.
#[derive(Clone, Copy)]
struct SelectionBytes {
    key: (u64, u64, u64),
    sizes: u64,
    bytes: Option<u64>,
    unknown_at: usize,
}

impl Default for ListState {
    fn default() -> Self {
        ListState {
            sorted: std::sync::Arc::from([]),
            sorted_at: None,
            positions: std::cell::OnceCell::new(),
            pending_hydrate: HashMap::new(),
            listing: None,
            removed: 0,
            sort_hydrated: None,
            selection_hydrated: None,
            visible_rows: 20,
            visible_items: 0,
            typed: None,
            typed_label: None,
            selection: Cell::new(None),
            selection_bytes: Cell::new(None),
        }
    }
}

impl ListState {
    /// Adopt the pane's order (`Pane::order`: the listing sorted, and the
    /// folders opened in place) when the listing's version, the opened
    /// folders or the spec changed.
    pub fn sync(&mut self, pane: &skwir_core::workspace::Pane) -> bool {
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
                .retain(|(listing, id), _| pane.listing_of(*id) == Some(*listing));
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
        self.positions = std::cell::OnceCell::new();
        true
    }

    pub fn version(&self) -> Option<u64> {
        self.sorted_at.map(|(v, _, _, _)| v)
    }

    fn reset(&mut self) {
        self.sorted = std::sync::Arc::from([]);
        self.sorted_at = None;
        self.positions = std::cell::OnceCell::new();
        self.pending_hydrate.clear();
        self.sort_hydrated = None;
        self.selection_hydrated = None;
        self.selection.set(None);
    }

    /// Whether the last `sync` adopted another listing than the one before.
    pub fn listing(&self) -> Option<u64> {
        self.listing
    }

    pub fn index_of(&self, id: EntryId) -> Option<usize> {
        // A short order is walked: a map is no cheaper to make.
        if self.sorted.len() <= 64 {
            return self.sorted.iter().position(|x| *x == id);
        }
        self.positions
            .get_or_init(|| {
                self.sorted
                    .iter()
                    .enumerate()
                    .map(|(ix, id)| (*id, ix))
                    .collect()
            })
            .get(&id)
            .copied()
    }
}

/// What the selection of `pane` amounts to, but for what it weighs
/// (`selection_bytes`). Walks the whole selection.
fn summarize(pane: &skwir_core::workspace::Pane) -> SelectionCtx {
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
            Some(_) => ctx.all_dirs = false,
            None => {}
        }
    }
    ctx
}

/// What the selected files of `pane` weigh, once every one's size is
/// known. A batch of metadata that leaves the file found unknown last
/// time unknown costs one look; one that told it looks on from there
/// (`Pane::selected_unknown_from`), and only a selection all known is
/// added up, so hydrating a big selection is not a walk of it per batch.
fn selection_bytes(
    pane: &skwir_core::workspace::Pane,
    key: (u64, u64, u64),
    last: &Cell<Option<SelectionBytes>>,
) -> Option<u64> {
    let sizes = pane.listing.size_version();
    let was = last.get();
    if let Some(was) = was
        && was.key == key
        && was.sizes == sizes
    {
        return was.bytes;
    }
    let from = was.map_or(0, |w| w.unknown_at);
    let (bytes, unknown_at) = if let Some(at) = pane.selected_unknown_from(from) {
        (None, at)
    } else {
        // Each file once: its names selected (hard links) are one file.
        let bytes =
            skwir_core::entry::weight(pane.selection.iter().filter_map(|id| pane.entry(id)));
        (Some(bytes), 0)
    };
    last.set(Some(SelectionBytes {
        key,
        sizes,
        bytes,
        unknown_at,
    }));
    bytes
}

impl Shell {
    /// Selection summary for `when` evaluation and the pane key context.
    /// Cached per pane on the selection's revision and what the summary
    /// reads of the listing (its names and kinds; its sizes for what the
    /// selection weighs, `selection_bytes`): with 100 000 entries
    /// selected, a frame must not walk them for every reader, nor a batch
    /// of metadata.
    pub fn selection_ctx(&self) -> SelectionCtx {
        let ws = self.app.workspace.read();
        let Some(pane) = ws.pane(self.pane) else {
            return SelectionCtx::default();
        };
        // Where the pane is, read every time: navigating replaces the
        // listing, whose version can then repeat the cached one.
        let folder = pane.location().is_some_and(skwir_core::Location::is_path);
        let place = &self.ui().place;
        let verbs = &place.verbs;
        let here = |ctx: SelectionCtx| SelectionCtx {
            folder,
            search: place.search,
            refs: verbs.remove.is_some(),
            drops: verbs.drop.is_some(),
            creates: verbs.create.is_some(),
            restores: verbs.restore.is_some(),
            ..ctx
        };
        let key = (
            pane.selection.revision(),
            pane.listing.names_version(),
            pane.tree_version(),
        );
        let list = &self.ui().list;
        let bytes = selection_bytes(pane, key, &list.selection_bytes);
        let cache = &list.selection;
        if let Some((at, ctx)) = cache.get()
            && at == key
        {
            return here(SelectionCtx { bytes, ..ctx });
        }
        let ctx = summarize(pane);
        cache.set(Some((key, ctx)));
        here(SelectionCtx { bytes, ..ctx })
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

    /// Sort this pane by `key` through the core command (same key again flips the direction).
    pub fn sort_by(&mut self, key: SortKey, cx: &mut Context<Self>) {
        self.dispatch_core(
            "sort",
            serde_json::json!({ "key": key, "pane": self.pane }),
            cx,
        );
    }

    fn dispatch_core(&mut self, name: &str, args: serde_json::Value, cx: &mut Context<Self>) {
        let inv = skwir_core::command::Invocation {
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
        self.ask_hydrate(ids, true);
    }

    /// Ask for the metadata of entries not drawn (`Pane::unknown`), after
    /// those that are.
    pub fn request_hydrate_off_screen(&mut self, ids: impl IntoIterator<Item = EntryId>) {
        self.ask_hydrate(ids, false);
    }

    /// Ask for the metadata of entries drawn from a listing the pane keeps
    /// beside the one it shows (`Pane::kept`: a column of the columns
    /// view), which its tags need.
    pub fn request_hydrate_kept(
        &mut self,
        location: &skwir_core::Location,
        ids: impl IntoIterator<Item = EntryId>,
    ) {
        let held: Vec<(u64, EntryId)> = {
            let ws = self.app.workspace.read();
            let Some(listing) = ws.pane(self.pane).and_then(|p| p.kept(location)) else {
                return;
            };
            ids.into_iter()
                .filter(|id| listing.get(*id).is_some_and(|e| !e.metadata.is_hydrated()))
                .map(|id| (listing.id(), id))
                .collect()
        };
        self.ask_hydrate_held(held, true);
    }

    fn ask_hydrate(&mut self, ids: impl IntoIterator<Item = EntryId>, on_screen: bool) {
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
        self.ask_hydrate_held(held, on_screen);
    }

    /// Ask for `held`, each entry with the listing that holds it, but what
    /// was asked already: on screen, once; off it, once, or once more
    /// when it comes on screen.
    fn ask_hydrate_held(&mut self, held: Vec<(u64, EntryId)>, on_screen: bool) {
        let pane = self.pane;
        let need: Vec<EntryId> = {
            let pending = &mut self.ui_mut().list.pending_hydrate;
            held.into_iter()
                .filter(|key| match pending.get_mut(key) {
                    Some(asked) if *asked || !on_screen => false,
                    Some(asked) => {
                        *asked = true;
                        true
                    }
                    None => {
                        pending.insert(*key, on_screen);
                        true
                    }
                })
                .map(|(_, id)| id)
                .collect()
        };
        if need.is_empty() {
            return;
        }
        tracing::debug!(n = need.len(), on_screen, "hydrate request");
        // Typed: a sort by size asks for a hundred thousand at once.
        if let Err(e) = self.app.dispatch_args(
            "hydrate",
            HydrateArgs {
                ids: need,
                pane: Some(pane),
                off_screen: !on_screen,
            },
        ) {
            tracing::warn!("hydrate: {e:#}");
        }
    }

    /// Ask for the metadata of the selected files whose size is not known
    /// yet, after what is drawn: until each has it, the selection's size
    /// is not said, rather than a part of it that grows as rows are drawn.
    /// Asked again only when the selection changes.
    pub(crate) fn sync_selection_hydration(&mut self) {
        let selection = self.selection_ctx();
        if selection.bytes.is_some() {
            return;
        }
        let pane = self.pane;
        let app = std::sync::Arc::clone(&self.app);
        let (revision, unknown) = {
            let ws = app.workspace.read();
            let Some(p) = ws.pane(pane) else { return };
            let revision = (p.listing.id(), p.selection.revision());
            if self.ui().list.selection_hydrated == Some(revision) {
                return;
            }
            let unknown: Vec<EntryId> = p
                .selection
                .iter()
                .filter(|id| {
                    p.entry(*id)
                        .is_some_and(|e| !e.is_dir() && !e.metadata.is_hydrated())
                })
                .collect();
            (revision, unknown)
        };
        self.ui_mut().list.selection_hydrated = Some(revision);
        self.request_hydrate_off_screen(unknown);
    }

    /// Under a sort by size, date or rating, ask for the metadata of every
    /// entry the pane shows: until it comes, an entry sorts after the rest
    /// (`SortKey::needs_metadata`). Asked again only when the entries or
    /// the sort change, never per frame.
    pub(crate) fn sync_sort_hydration(&mut self) {
        let pane = self.pane;
        let app = std::sync::Arc::clone(&self.app);
        let (key, unknown) = {
            let ws = app.workspace.read();
            let Some(p) = ws.pane(pane) else { return };
            // Once listed: while a folder streams in, its names change
            // with every batch, and each look costs a pass over them.
            if !p.sort.key.needs_metadata() || !p.listing.is_complete() {
                return;
            }
            let key = (
                p.listing.id(),
                p.listing.names_version(),
                p.tree_members_version(),
                p.sort,
                p.filter_version(),
            );
            if self.ui().list.sort_hydrated == Some(key) {
                return;
            }
            (key, p.unknown())
        };
        self.ui_mut().list.sort_hydrated = Some(key);
        self.request_hydrate_off_screen(unknown);
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
        if let Some(target) = self.select_at(target, step, window, cx) {
            self.with_view(|v, _| v.scroll_to(target));
            cx.notify();
        }
    }

    /// Move the cursor to `target` (clamped) as `step` says, and say
    /// where it went (`None`: the order is empty); bringing it into view
    /// is the caller's.
    fn select_at(
        &mut self,
        target: isize,
        step: Step,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<usize> {
        let len = self.ui().list.sorted.len();
        if len == 0 {
            return None;
        }
        let target = target.clamp(0, len as isize - 1) as usize;
        let id = self.ui().list.sorted[target];
        let (mode, at) = step.select();
        let args = SelectArgs {
            ids: vec![id],
            mode,
            pane: Some(self.pane),
            // The row is known: a key held down through a big folder never
            // has the core look for it.
            at: at.then_some(target),
            ..SelectArgs::default()
        };
        self.run_args("select", args, window, cx);
        Some(target)
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
        let list = &self.ui().list;
        let sorted = &list.sorted;
        let len = sorted.len();
        let start = from
            .or(pane.selection.cursor)
            .and_then(|c| list.index_of(c));
        // The pattern folded once, and a name compared without a copy of
        // its own: lowercasing both for each entry at each keystroke.
        let filter = skwir_core::workspace::NameFilter::new(pattern);
        (1..=len)
            .map(|step| match (start, backward) {
                (Some(s), false) => (s + step) % len,
                (Some(s), true) => (s + len - step % len) % len,
                (None, false) => step - 1,
                (None, true) => len - step,
            })
            .find(|ix| {
                pane.entry(sorted[*ix])
                    .is_some_and(|e| filter.holds(&e.name))
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
        window.focus(&self.ui().focus, cx);
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
                } else if crate::keymap::secondary(&ev.modifiers) && ev.modifiers.shift {
                    // A range added to what is selected, not in its place.
                    self.select_ids(vec![id], "add_range", window, cx);
                } else if crate::keymap::secondary(&ev.modifiers) {
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
        // Space only goes on with a name: here when no binding took it.
        if ch == " " {
            self.type_space(window, cx);
            return;
        }
        if ch.is_empty() || ch.chars().any(char::is_control) {
            return;
        }
        let now = Instant::now();
        // The listing shown now: one a key just opened, even before the
        // next sync adopts it.
        let Some((listing, cursor)) = self
            .app
            .workspace
            .read()
            .pane(self.pane)
            .map(|p| (p.listing.id(), p.selection.cursor))
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
                    input.focus(window, cx);
                });
                cx.notify();
                return;
            }
            let begun = l.typed.is_none();
            let typed = l.typed.get_or_insert_with(|| Typed::new(listing, now));
            typed.push(ch, now);
            typed.from = cursor;
            typed.past = begun || typed.repeats();
        }
        let Some((found, whole)) = self.seek_typed() else {
            return;
        };
        self.typed_reached(found, whole, window, cx);
    }

    /// Space typed while a name is: it goes on with the name when a name in
    /// the listing goes on so ("my f" reaching "My file.txt"), and says
    /// whether it did. Else it is left to its binding (Peek): Space begins
    /// no name, and ends none that no name goes on with. Asked before the
    /// binding runs (`pane_container`).
    fn type_space(&mut self, window: &mut Window, cx: &mut Context<Self>) -> bool {
        if !self.keys.typeahead {
            return false;
        }
        let now = Instant::now();
        let reached = {
            let ws = self.app.workspace.read();
            let Some(pane) = ws.pane(self.pane) else {
                return false;
            };
            let list = &self.ui().list;
            let Some(typed) = list.typed.as_ref().filter(|typed| {
                typed.goes_on(pane.listing.id(), now) && list.listing == Some(typed.listing)
            }) else {
                return false;
            };
            let prefix = skwir_core::collate::Prefix::new(&format!("{} ", typed.text));
            let cursor = pane.selection.cursor;
            seek_from(&list.sorted, cursor, false, |id| {
                pane.entry(id).is_some_and(|e| prefix.begins(&e.name))
            })
            .map(|ix| (ix, cursor))
        };
        let Some((ix, cursor)) = reached else {
            return false;
        };
        if let Some(typed) = self.ui_mut().list.typed.as_mut() {
            typed.push(" ", now);
            typed.from = cursor;
            typed.past = false;
        }
        let whole = self.typed_listing_whole();
        self.typed_reached(Some(ix), whole, window, cx);
        true
    }

    /// Where the name typed leads: the first entry in the order shown whose
    /// name begins with it (`Typed::prefix`), going on from where its last
    /// character was typed (`Typed::from`), and whether that order is the
    /// whole listing it was typed in. A folder just opened is adopted at
    /// the next sync, and streams: until it is whole, an entry the name
    /// reaches first may come.
    fn seek_typed(&self) -> Option<(Option<usize>, bool)> {
        let ws = self.app.workspace.read();
        let pane = ws.pane(self.pane)?;
        let list = &self.ui().list;
        let typed = list.typed.as_ref()?;
        if pane.listing.id() != typed.listing || list.listing != Some(typed.listing) {
            return Some((None, false));
        }
        let prefix = typed.prefix();
        let found = seek_from(&list.sorted, typed.from, typed.past, |id| {
            pane.entry(id).is_some_and(|e| prefix.begins(&e.name))
        });
        let whole = list.version() == Some(pane.listing.version())
            && (pane.listing.is_complete() || pane.listing.error().is_some());
        Some((found, whole))
    }

    /// The entry a name typed reached, selected and brought to the middle
    /// of the view, so what is around it shows; "No match" when the listing
    /// is whole and none was; the label kept until the pause ends.
    fn typed_reached(
        &mut self,
        found: Option<usize>,
        whole: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let Some(ix) = found
            && let Some(ix) = self.select_at(ix as isize, Step::Select, window, cx)
        {
            self.with_view(|view, _| view.scroll_to_middle(ix));
        }
        if let Some(typed) = self.ui_mut().list.typed.as_mut() {
            typed.missed = found.is_none() && whole;
        }
        self.typed_waits(whole);
        // The label goes once the pause has ended: one redraw then.
        self.ui_mut().list.typed_label = Some(cx.spawn(async move |this, cx| {
            cx.background_executor().timer(TYPING_PAUSE).await;
            this.update(cx, |_, cx| cx.notify()).ok();
        }));
        cx.notify();
    }

    /// What is being typed to reach a name, at the foot of the pane until
    /// the pause ends: what names must begin with (`Typed::sought`), a
    /// caret after it (a space typed shows), and "No match" once the
    /// listing, whole, has none. A status line read when asked, never
    /// announced: the entry reached is said as the cursor moves to it.
    fn typed_label(&self, cx: &Context<Self>) -> Option<AnyElement> {
        let list = &self.ui().list;
        let typed = list.typed.as_ref()?;
        if list.listing != Some(typed.listing) || !typed.goes_on(typed.listing, Instant::now()) {
            return None;
        }
        let theme = cx.global::<Theme>();
        let sought = typed.sought().to_owned();
        let said = self.a11y.then(|| {
            if typed.missed {
                format!("{sought}: No match")
            } else {
                sought.clone()
            }
        });
        let pill = div()
            .id(("typed", self.pane.0 as usize))
            .role(gpui::Role::Status)
            .when_some(said, StatefulInteractiveElement::aria_label)
            .h(px(28.))
            .max_w_full()
            .px(px(12.))
            .flex()
            .items_center()
            .gap(px(8.))
            .overflow_hidden()
            .whitespace_nowrap()
            .rounded_full()
            .border_1()
            .border_color(theme.border)
            .bg(theme.surface)
            .shadow_md()
            .text_color(theme.text)
            .child(
                div()
                    .flex()
                    .items_center()
                    .child(sought)
                    .child(div().flex_none().w(px(2.)).h(px(16.)).bg(theme.accent)),
            )
            .when(typed.missed, |pill| {
                pill.child(div().text_color(theme.text_muted).child("No match"))
            });
        Some(
            div()
                .absolute()
                .bottom(px(12.))
                .left_0()
                .right_0()
                .px(px(16.))
                .flex()
                .justify_center()
                .child(pill)
                .into_any_element(),
        )
    }

    /// Whether the pane's listing is whole: the last look then settles.
    fn typed_listing_whole(&self) -> bool {
        let ws = self.app.workspace.read();
        ws.pane(self.pane).is_some_and(|pane| {
            self.ui().list.version() == Some(pane.listing.version())
                && (pane.listing.is_complete() || pane.listing.error().is_some())
        })
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
        let names = {
            let ws = self.app.workspace.read();
            ws.pane(self.pane)
                .filter(|p| !p.sort.key.needs_metadata())
                .map(|p| p.listing.names_version())
        };
        if names.is_some()
            && self.ui().list.typed.as_ref().and_then(|t| t.looked) == names
            && !self.typed_listing_whole()
        {
            return;
        }
        if let Some(typed) = self.ui_mut().list.typed.as_mut() {
            typed.looked = names;
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
            self.with_view(|view, _| view.scroll_to_middle(ix));
            cx.notify();
        }
        if let Some(typed) = self.ui_mut().list.typed.as_mut() {
            typed.missed = found.is_none() && whole;
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
        window.focus(&self.ui().focus, cx);
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
            keeps: kept.iter().copied().collect(),
            kept,
            sent: None,
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
            // In the pane the band began in, whichever is active now (a
            // key or the other pane's menu may have moved it meanwhile).
            let pane = drag.pane;
            self.with_pane(pane, |shell| {
                let sorted = std::sync::Arc::clone(&shell.ui().list.sorted);
                let covered: Vec<EntryId> = covered
                    .iter()
                    .filter_map(|index| sorted.get(*index).copied())
                    .collect();
                let Some(drag) = shell.marquee.as_mut() else {
                    return;
                };
                // Its first move replaces the selection with what was kept
                // and what it covers; each after sends what it changed
                // (`band_change`), never the whole selection again.
                let args = match drag.sent.as_mut() {
                    None => {
                        drag.sent = Some(
                            covered
                                .iter()
                                .copied()
                                .filter(|id| !drag.keeps.contains(id))
                                .collect(),
                        );
                        let mut ids = drag.kept.clone();
                        ids.extend(covered);
                        SelectArgs {
                            ids,
                            mode: SelectMode::Replace,
                            pane: Some(pane),
                            ..SelectArgs::default()
                        }
                    }
                    Some(sent) => {
                        let (ids, removed) =
                            crate::elements::marquee::band_change(&drag.keeps, sent, &covered);
                        SelectArgs {
                            ids,
                            removed,
                            mode: SelectMode::Change,
                            pane: Some(pane),
                            ..SelectArgs::default()
                        }
                    }
                };
                shell.run_args("select", args, window, cx);
            });
        }
        cx.notify();
    }

    /// The band let go: what it selected is settled as one band replacing
    /// the selection would have it, what was kept first, then what it
    /// covers in the order shown (which the changes it sent as it moved
    /// may have mixed): what is read in the selection's order (a bulk
    /// rename's numbers, a drag's label) reads it so.
    pub fn marquee_end(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(drag) = self.marquee.take() else {
            return;
        };
        if drag.sent.is_some() {
            let pane = drag.pane;
            self.with_pane(pane, |shell| {
                let sorted = &shell.ui().list.sorted;
                let mut ids = drag.kept;
                ids.extend(
                    drag.covered
                        .iter()
                        .filter_map(|index| sorted.get(*index).copied()),
                );
                let args = SelectArgs {
                    ids,
                    mode: SelectMode::Replace,
                    pane: Some(pane),
                    ..SelectArgs::default()
                };
                shell.run_args("select", args, window, cx);
            });
        }
        cx.notify();
    }

    /// Wrap a view's element in the pane container: focus, key context,
    /// type-ahead, empty-area context menu, external drops. It holds the
    /// keyboard focus, so it is what assistive technology hears first: the
    /// view's role (`PaneView::role`), named by the location, described
    /// by what the pane says in place of entries (`said`) when it lists
    /// none.
    pub fn pane_container(
        &mut self,
        content: AnyElement,
        said: Option<String>,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let tint = cx.global::<Theme>().drop_target;
        let key_context = self.pane_key_context();
        let pane = self.pane;
        let typed = self.typed_label(cx);
        let role = match &said {
            Some(_) => gpui::Role::Group,
            None => self
                .ui()
                .views
                .get(self.ui().view_id)
                .map_or(gpui::Role::Group, |view| view.role()),
        };
        // A list box says how many entries it holds; a tree's are counted
        // by level.
        let count = (self.a11y && role == gpui::Role::ListBox).then(|| self.ui().list.sorted.len());
        div()
            .id(("pane", pane.0 as usize))
            .role(role)
            // Entries are picked with Ctrl and Shift as with the pointer.
            .when(
                matches!(role, gpui::Role::ListBox | gpui::Role::Tree),
                StatefulInteractiveElement::aria_multiselectable,
            )
            .when(self.a11y, |d| d.aria_label(self.ui().place.title.clone()))
            .when_some(count, StatefulInteractiveElement::aria_size_of_set)
            .when_some(said, StatefulInteractiveElement::aria_description)
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
            // Space goes on with a name being typed that a name goes on
            // with, before its binding here (Peek) runs.
            .capture_action(
                cx.listener(move |this, _: &keymap::RunCommand, window, cx| {
                    let space = window.key_down_event().is_some_and(|ev| {
                        !ev.is_held
                            && ev.keystroke.key == "space"
                            && ev.keystroke.modifiers.number_of_modifiers() == 0
                    });
                    if space && this.with_pane(pane, |this| this.type_space(window, cx)) {
                        cx.stop_propagation();
                    }
                }),
            )
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(move |this, _, _, cx| this.activate_pane(pane, cx)),
            )
            .on_mouse_down(
                MouseButton::Right,
                cx.listener(move |this, ev: &MouseDownEvent, window, cx| {
                    this.activate_pane(pane, cx);
                    window.focus(&this.ui().focus, cx);
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
            .children(typed)
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
    /// Each step selects as it always did (the modes `select_ids` named),
    /// and a range and a following move say the row they reach.
    #[test]
    fn a_step_selects_as_it_did_and_says_its_row_where_asked() {
        let said = |step: Step| {
            let (mode, at) = step.select();
            (serde_json::to_value(mode).unwrap(), at)
        };
        assert_eq!(said(Step::Select), (serde_json::json!("replace"), false));
        assert_eq!(said(Step::Extend), (serde_json::json!("range_to"), true));
        assert_eq!(said(Step::Cursor), (serde_json::json!("cursor"), false));
        assert_eq!(said(Step::Follow), (serde_json::json!("follow"), true));
    }

    use std::time::{Duration, Instant};

    use super::{Step, TYPING_PAUSE, Typed, find_matches, seek_from};

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

    fn typed(text: &str) -> Typed {
        let now = Instant::now();
        let mut typed = Typed::new(1, now);
        for ch in text.chars() {
            typed.push(ch.encode_utf8(&mut [0; 4]), now);
        }
        typed
    }

    #[test]
    fn a_name_typed_begins_names_whatever_their_case_and_accents() {
        let begins = |text: &str, name: &str| typed(text).prefix().begins(name.as_ref());
        assert!(begins("PiC", "pictures"));
        assert!(begins("PiC", "PICTURES"));
        assert!(!begins("PiC", "pi"));
        assert!(!begins("PiC", "papers"));
        assert!(begins("É", "été 2026"));
        assert!(begins("ecole", "École"));
        assert!(begins("", "anything"));
    }

    /// One letter typed again and again looks for that letter, each press
    /// reaching the next name; any other name typed is looked for whole.
    #[test]
    fn a_letter_typed_again_looks_for_itself_once() {
        assert_eq!(typed("d").sought(), "d");
        assert_eq!(typed("ddd").sought(), "d");
        assert_eq!(typed("dD").sought(), "d");
        assert!(typed("dd").repeats());
        assert!(!typed("d").repeats());
        assert_eq!(typed("ddo").sought(), "ddo");
        assert!(!typed("ddo").repeats());
        assert_eq!(typed("éé").sought(), "é");
        assert_eq!(typed("my ").sought(), "my ");
    }

    /// The look goes on from where it began, past it or not, round the
    /// end; with nothing to begin from, from the first.
    #[test]
    fn a_look_goes_on_from_where_it_began_and_round_the_end() {
        let order = [10, 11, 12, 13, 14];
        let d = |n: i32| n == 11 || n == 13;
        assert_eq!(seek_from(&order, None, false, d), Some(1));
        assert_eq!(seek_from(&order, None, true, d), Some(1));
        // On a match: stays unless passed over.
        assert_eq!(seek_from(&order, Some(11), false, d), Some(1));
        assert_eq!(seek_from(&order, Some(11), true, d), Some(3));
        // Past the last match: round the end.
        assert_eq!(seek_from(&order, Some(13), true, d), Some(1));
        assert_eq!(seek_from(&order, Some(14), false, d), Some(1));
        // Between matches.
        assert_eq!(seek_from(&order, Some(12), true, d), Some(3));
        // One match, passed over: it again.
        let one = |n: i32| n == 12;
        assert_eq!(seek_from(&order, Some(12), true, one), Some(2));
        // Begun from something no longer in the order: from the first.
        assert_eq!(seek_from(&order, Some(99), true, d), Some(1));
        // Nothing matches.
        assert_eq!(seek_from(&order, Some(12), true, |_| false), None);
        assert_eq!(seek_from(&[] as &[i32], None, false, d), None);
    }
}
