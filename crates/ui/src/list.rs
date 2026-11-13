//! Pane state shared by every view: the cached sorted order, hydration
//! bookkeeping, selection and cursor interaction, type-ahead, and the pane
//! container (focus, key context, external drops).

use crate::sizes::sized;
use std::cell::Cell;
use std::collections::HashMap;
use std::path::PathBuf;
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

/// What arrives being placed (`ListState::sync_arriving`): before the
/// entry at `at`, `depth` folders opened deep, its place among what its
/// folder brings, and its group.
struct Placing {
    at: usize,
    depth: usize,
    among: usize,
    group: Option<usize>,
    /// Under the heading of a group no file is in yet (`RowMap::fresh`).
    fresh: Option<usize>,
    arriving: skwir_core::op::arriving::Arriving,
}

pub struct ListState {
    /// Entry ids in display order, from the core's cached sort.
    pub sorted: std::sync::Arc<[EntryId]>,
    /// Where `sorted`'s groups' headings stand among its entries, for the
    /// views that draw them (ADR-130, 9): made with `sorted`, from the
    /// pane as it stood then.
    pub rows: crate::views::groups::RowMap,
    /// The heading the keys are on, if any (ADR-131, 1).
    pub heading: Option<HeadingStop>,
    /// The groups `rows` was made with, kept to place what arrives among
    /// them (`sync_arriving`).
    groups: std::sync::Arc<[skwir_core::grouping::Span]>,
    /// What plans bring into the pane's folder and has not landed (ADR-135),
    /// once it lasted `work::SHOWN_AFTER`, as `rows` names each
    /// (`Row::Arriving`): drawn where it will land, never an entry.
    pub arriving: Vec<skwir_core::op::arriving::Arriving>,
    /// How many folders opened in place deep each of `arriving` is: 0 in
    /// the pane's folder.
    pub arriving_depth: Vec<usize>,
    /// Where each of `arriving` was placed: before which position, in
    /// which group (what the board files it under).
    pub arriving_placed: Vec<crate::views::groups::Placed>,
    /// The entries listed that something arriving lands on (a folder merged
    /// into, a file written in place, one placed whose step has not ended):
    /// drawn with how far it has come.
    pub arriving_on: HashMap<EntryId, skwir_core::op::arriving::Arriving>,
    /// What `rows` placed what arrives for: the order, the groups, and
    /// what arrives, by identity.
    arriving_for: Option<(usize, usize, Vec<usize>)>,
    /// Since when the files of the folder left are drawn while the one
    /// gone to is filled elsewhere (ADR-120, amended: `holds_left`), and
    /// whether a command ended that.
    held_since: Option<std::time::Instant>,
    hold_ended: bool,
    sorted_at: Option<(u64, u64, SortSpec, (u64, u64))>,
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
    /// What `sync_selection_hydration` last asked for: the listing and the
    /// selection's revision.
    selection_hydrated: Option<(u64, u64)>,
    /// What `sync_lent_hydration` last looked at: the selection's revision
    /// and what was lent (`Pane::lent_version`).
    lent_hydrated: Option<(u64, u64)>,
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

/// The heading the keys are on (ADR-131, 1): its group, by where it is
/// among them and by itself, found again as the order changes; and the
/// cursor the keys left there, whose moving any other way lets it go. A
/// heading is never selected nor acted on as a file.
#[derive(Clone, Debug)]
pub struct HeadingStop {
    pub group: usize,
    pub of: Option<std::sync::Arc<skwir_core::grouping::Group>>,
    pub cursor: Option<EntryId>,
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
            rows: crate::views::groups::RowMap::default(),
            heading: None,
            groups: std::sync::Arc::from([]),
            arriving: Vec::new(),
            arriving_depth: Vec::new(),
            arriving_placed: Vec::new(),
            arriving_on: HashMap::new(),
            arriving_for: None,
            held_since: None,
            hold_ended: false,
            sorted_at: None,
            positions: std::cell::OnceCell::new(),
            pending_hydrate: HashMap::new(),
            listing: None,
            removed: 0,
            selection_hydrated: None,
            lent_hydrated: None,
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
        // The keys on a heading: let go once the cursor moved by any other
        // way (a press, a command, a name typed).
        if self
            .heading
            .as_ref()
            .is_some_and(|stop| stop.cursor != pane.selection.cursor)
        {
            self.heading = None;
        }
        let listing = &pane.listing;
        if self.listing != Some(listing.id()) {
            if self.holds_left(listing) {
                return false;
            }
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
            (pane.filter_version(), pane.folds_version()),
        );
        if self.sorted_at == Some(at) {
            return false;
        }
        self.sorted = pane.order();
        self.groups = pane.groups();
        self.rows = crate::views::groups::RowMap::new(self.groups.clone(), self.sorted.len());
        // What arrives is placed again among the rows made anew.
        self.arriving_for = None;
        // The heading the keys are on, found again among the groups.
        if let Some(stop) = &mut self.heading {
            match self
                .rows
                .spans()
                .iter()
                .position(|span| skwir_core::grouping::same(span.group.as_ref(), stop.of.as_ref()))
            {
                Some(group) => stop.group = group,
                None => self.heading = None,
            }
        }
        self.sorted_at = Some(at);
        self.positions = std::cell::OnceCell::new();
        true
    }

    pub fn version(&self) -> Option<u64> {
        self.sorted_at.map(|(v, _, _, _)| v)
    }

    /// Whether the files of the folder left stay drawn (ADR-120, amended):
    /// `listing`, the one gone to, is filled elsewhere and holds nothing
    /// yet, and it was gone to less than `tabs::HOLD` ago, as a tab left
    /// stays drawn while the one shown gets ready, no command having ended
    /// that. Then the window goes from one folder to the next with no
    /// empty frame between, the address and the trail having changed at
    /// once; past it, the folder gone to is shown empty, "Loading…" said
    /// in time. Starts the wait.
    fn holds_left(&mut self, listing: &skwir_core::location::Listing) -> bool {
        if !listing.is_held() || self.listing.is_none() || self.sorted.is_empty() || self.hold_ended
        {
            return false;
        }
        let since = *self.held_since.get_or_insert_with(std::time::Instant::now);
        since.elapsed() < crate::tabs::HOLD
    }

    /// Whether it draws the files of the folder left (`holds_left`).
    pub fn holding(&self) -> bool {
        self.held_since.is_some() && !self.hold_ended
    }

    /// A command was run: what it acts on is the folder gone to, which
    /// the next sync shows, whatever it holds yet.
    pub fn end_hold(&mut self) {
        if self.held_since.is_some() {
            self.hold_ended = true;
        }
    }

    fn reset(&mut self) {
        self.sorted = std::sync::Arc::from([]);
        self.rows = crate::views::groups::RowMap::default();
        self.heading = None;
        self.groups = std::sync::Arc::from([]);
        self.arriving.clear();
        self.arriving_depth.clear();
        self.arriving_placed.clear();
        self.arriving_on.clear();
        self.arriving_for = None;
        self.held_since = None;
        self.hold_ended = false;
        self.sorted_at = None;
        self.positions = std::cell::OnceCell::new();
        self.pending_hydrate.clear();
        self.selection_hydrated = None;
        self.lent_hydrated = None;
        self.selection.set(None);
    }

    /// Whether the last `sync` adopted another listing than the one before.
    pub fn listing(&self) -> Option<u64> {
        self.listing
    }

    /// Take what plans bring into the pane's folder, and into each folder
    /// it opened in place, as the executor says it now (ADR-135): each item
    /// asked `work::SHOWN_AFTER` ago or more, placed among the rows where it
    /// will land, or on the entry already listed under its name. Placed
    /// again only when the order, its groups or what arrives changed;
    /// otherwise how far each has come is all that is taken. Nothing for a
    /// pane that shows no folder, or whose folder is still being read.
    /// Whether anything drawn changed, and when the next item waiting to be
    /// shown is due.
    pub fn sync_arriving(
        &mut self,
        pane: &skwir_core::workspace::Pane,
        executor: &skwir_core::op::Executor,
        now: Instant,
    ) -> (bool, Option<Instant>) {
        use skwir_core::op::arriving::Arriving;
        // Where something may arrive: the pane's folder, then each folder
        // it opened in place, by the entry it opens at.
        let mut sources: Vec<(Option<EntryId>, &skwir_core::location::Listing, PathBuf)> =
            Vec::new();
        if !pane.listing.is_held()
            && let Some(folder) = pane.location().and_then(skwir_core::Location::path)
        {
            sources.push((None, &pane.listing, folder));
            for (opened, listing) in pane.opened_listings() {
                if let Some(path) = opened.location.path() {
                    sources.push((Some(opened.at), listing, path));
                }
            }
        }
        let mut due: Option<Instant> = None;
        let mut ghosts: Vec<(usize, Arriving)> = Vec::new();
        let mut on = HashMap::new();
        for (source, (_, listing, folder)) in sources.iter().enumerate() {
            for arriving in executor.arriving_in(folder) {
                let shown_at = arriving.arrival.asked + crate::work::SHOWN_AFTER;
                if shown_at > now {
                    due = Some(due.map_or(shown_at, |due| due.min(shown_at)));
                    continue;
                }
                match listing.find_by_name(&arriving.arrival.name) {
                    Some(entry) => {
                        on.insert(entry.id, arriving);
                    }
                    None => ghosts.push((source, arriving)),
                }
            }
        }
        let on_moved = on.len() != self.arriving_on.len()
            || on.iter().any(|(id, a)| {
                self.arriving_on
                    .get(id)
                    .is_none_or(|was| was.state != a.state || was.paused != a.paused)
            });
        self.arriving_on = on;
        let key = |a: &Arriving| std::sync::Arc::as_ptr(&a.arrival).addr();
        let placed_for = (
            std::sync::Arc::as_ptr(&self.sorted).cast::<()>().addr(),
            std::sync::Arc::as_ptr(&self.groups).cast::<()>().addr(),
            ghosts.iter().map(|(_, a)| key(a)).collect::<Vec<usize>>(),
        );
        // Nothing arrives, nothing did: the rows stay as made.
        if ghosts.is_empty() && self.arriving.is_empty() {
            self.arriving_for = Some(placed_for);
            return (on_moved, due);
        }
        if self.arriving_for.as_ref() == Some(&placed_for) {
            // The same, where they were: how far each has come.
            let now: HashMap<usize, &Arriving> = ghosts.iter().map(|(_, a)| (key(a), a)).collect();
            let mut moved = on_moved;
            for mine in &mut self.arriving {
                if let Some(new) = now.get(&key(mine))
                    && (mine.state != new.state || mine.paused != new.paused)
                {
                    mine.state = new.state;
                    mine.paused = new.paused;
                    moved = true;
                }
            }
            return (moved, due);
        }
        let was_drawn = !self.arriving.is_empty();
        self.arriving_for = Some(placed_for);
        // Each source's placed among its own listing, then where that is
        // among the rows: the pane's own at the top level, an opened
        // folder's under it, one level deeper, in its group.
        let mut kept: Vec<Placing> = Vec::new();
        // The groups no file is in yet that what arrives will be in, each
        // once, where it stands among the pane's (`Pane::fresh_before`).
        let mut fresh: Vec<crate::views::groups::Fresh> = Vec::new();
        for (source, (opened_at, listing, _)) in sources.iter().enumerate() {
            let mine: Vec<Arriving> = ghosts
                .iter()
                .filter(|(s, _)| *s == source)
                .map(|(_, a)| a.clone())
                .collect();
            if mine.is_empty() {
                continue;
            }
            let likes: Vec<&skwir_core::Entry> = mine.iter().map(|a| &a.arrival.like).collect();
            let stands = pane.stands_in(listing, &likes);
            let top = pane.sorted_of(listing);
            let (end, depth, group_at) = match opened_at {
                None => (self.sorted.len(), 0, None),
                // An opened folder not shown (filtered out, its group
                // folded) shows nothing of what arrives in it.
                Some(at) => {
                    let Some(folder) = self.index_of(*at) else {
                        continue;
                    };
                    let depth = pane.depth(*at) + 1;
                    (self.below_end(pane, folder, depth), depth, Some(folder))
                }
            };
            let at_of = self.positions_of_listed(&top, &stands, end);
            for (arriving, stand) in mine.into_iter().zip(stands) {
                let Some(stand) = stand else {
                    continue;
                };
                let at = at_of.get(&stand.at).copied().unwrap_or(end);
                let group = match group_at {
                    None => self.group_for(at, stand.group.as_ref()),
                    Some(folder) => self.rows.group_of(folder),
                };
                // Grouped, in a group none of the pane's is: its own.
                let in_fresh = match (&stand.group, group) {
                    (Some(new), None) if group_at.is_none() && self.rows.grouped() => Some(
                        fresh
                            .iter()
                            .position(|f| skwir_core::grouping::same(Some(&f.group), Some(new)))
                            .unwrap_or_else(|| {
                                fresh.push(crate::views::groups::Fresh {
                                    before: pane.fresh_before(new, &self.groups),
                                    group: std::sync::Arc::clone(new),
                                });
                                fresh.len() - 1
                            }),
                    ),
                    _ => None,
                };
                kept.push(Placing {
                    at,
                    depth,
                    among: stand.among,
                    group,
                    fresh: in_fresh,
                    arriving,
                });
            }
        }
        // Before the same entry, the deeper first: what arrives at the end
        // of a folder opened comes before what goes after it.
        kept.sort_by_key(|p| (p.at, std::cmp::Reverse(p.depth), p.among));
        let placed: Vec<crate::views::groups::Placed> = kept
            .iter()
            .map(|p| crate::views::groups::Placed {
                at: p.at,
                group: p.group,
                fresh: p.fresh,
            })
            .collect();
        self.arriving_depth = kept.iter().map(|p| p.depth).collect();
        self.arriving = kept.into_iter().map(|p| p.arriving).collect();
        self.rows = crate::views::groups::RowMap::arriving(
            self.groups.clone(),
            self.sorted.len(),
            &placed,
            &fresh,
        );
        self.arriving_placed = placed;
        (was_drawn || !self.arriving.is_empty() || on_moved, due)
    }

    /// Where what is below the folder at `folder` ends among what the pane
    /// shows, its entries `depth` deep: the first entry after it no deeper
    /// than the folder.
    fn below_end(&self, pane: &skwir_core::workspace::Pane, folder: usize, depth: usize) -> usize {
        let mut at = folder + 1;
        while let Some(id) = self.sorted.get(at)
            && pane.depth(*id) >= depth
        {
            at += 1;
        }
        at
    }

    /// Where each place of a listing's order (`top`) that `stands` names is
    /// in what the pane shows (`sorted`): the first entry from there shown
    /// (the filter and the folds leave some out), `end` past the last. One
    /// walk from the last place asked to the first, each stopping where the
    /// one after it found its entry.
    fn positions_of_listed(
        &self,
        top: &[EntryId],
        stands: &[Option<skwir_core::workspace::Stand>],
        end: usize,
    ) -> HashMap<usize, usize> {
        let mut ats: Vec<usize> = stands.iter().flatten().map(|stand| stand.at).collect();
        ats.sort_unstable_by(|a, b| b.cmp(a));
        ats.dedup();
        let flat = std::ptr::eq(top, &raw const self.sorted[..]);
        let mut found = HashMap::with_capacity(ats.len());
        let (mut known_from, mut known) = (top.len(), end);
        for at in ats {
            let position = if flat {
                at.min(end)
            } else {
                let mut j = at;
                loop {
                    if j >= known_from {
                        break known;
                    }
                    if let Some(position) = top.get(j).and_then(|id| self.index_of(*id)) {
                        break position;
                    }
                    j += 1;
                }
            };
            found.insert(at, position);
            (known_from, known) = (at, position);
        }
        found
    }

    /// The group what arrives before the entry at `at` is drawn in, as the
    /// pane would group it (`group`): the one holding that entry, or the
    /// one before it ends at; none where it would be in neither (a group
    /// no file is in yet).
    fn group_for(
        &self,
        at: usize,
        group: Option<&std::sync::Arc<skwir_core::grouping::Group>>,
    ) -> Option<usize> {
        let rows = &self.rows;
        if !rows.grouped() {
            return None;
        }
        let same = |n: usize| skwir_core::grouping::same(rows.spans()[n].group.as_ref(), group);
        rows.group_of(at).filter(|n| same(*n)).or_else(|| {
            at.checked_sub(1)
                .and_then(|before| rows.group_of(before))
                .filter(|n| same(*n))
        })
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

    /// Ask for the metadata of entries not drawn, after
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
                .filter(|id| listing.get(*id).is_some_and(|e| !e.metadata.is_filled()))
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
                .filter(|id| p.entry(*id).is_some_and(|e| !e.metadata.is_filled()))
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

    /// Ask for what a listing would know of the selected entries a view
    /// lent the pane (ADR-116), read by their paths, and read again once
    /// lent again seen otherwise (what drew them again saw them change):
    /// what drew them (a scan) read no more than their kind. On screen:
    /// they are the selection, which the preview and Properties show.
    /// Asked again only when the selection or what is lent changes, which
    /// a reading never does (`Pane::lent_version`).
    pub(crate) fn sync_lent_hydration(&mut self) {
        let pane = self.pane;
        let app = std::sync::Arc::clone(&self.app);
        let (key, unread) = {
            let ws = app.workspace.read();
            let Some(p) = ws.pane(pane) else { return };
            if !p.has_lent() {
                return;
            }
            let key = (p.selection.revision(), p.lent_version());
            if self.ui().list.lent_hydrated == Some(key) {
                return;
            }
            let unread: Vec<EntryId> = p
                .selection
                .iter()
                .filter(|id| p.lent_to_read(*id))
                .collect();
            (key, unread)
        };
        self.ui_mut().list.lent_hydrated = Some(key);
        if unread.is_empty() {
            return;
        }
        if let Err(e) = self.app.dispatch_args(
            "hydrate",
            HydrateArgs {
                ids: unread,
                pane: Some(pane),
                off_screen: false,
            },
        ) {
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

    /// Whether `id` is an entry a view lent the pane (ADR-116).
    pub fn is_lent(&self, id: EntryId) -> bool {
        let ws = self.app.workspace.read();
        ws.pane(self.pane).is_some_and(|p| p.is_lent(id))
    }

    /// Move the cursor to `id` as `step` says, where the pane's order
    /// does not say where it is (an entry a view lent, ADR-116, or a view
    /// that moves by what it draws): a range to it adds it.
    pub fn select_entry(
        &mut self,
        id: EntryId,
        step: Step,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let (mode, _) = step.select();
        let args = SelectArgs {
            ids: vec![id],
            mode,
            pane: Some(self.pane),
            ..SelectArgs::default()
        };
        self.run_args("select", args, window, cx);
        cx.notify();
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
        self.ui_mut().list.heading = None;
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

    /// `ui.select_group`: select what a group's heading counts (ADR-130,
    /// 12), group `group` of what the pane draws, else the cursor's; with
    /// `add`, beside what is selected. In the tree, the group's own: not
    /// the entries of a folder opened in place under it. From the first to
    /// the last, as a band over them selects, the view left where it is.
    pub fn select_group(
        &mut self,
        group: Option<usize>,
        add: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let rows = self.ui().list.rows.clone();
        let Some(span) = group
            .or_else(|| rows.group_of(self.cursor_index()?))
            .and_then(|n| rows.spans().get(n))
        else {
            return;
        };
        let sorted = std::sync::Arc::clone(&self.ui().list.sorted);
        let Some(group) = sorted.get(span.start..span.start + span.len) else {
            return;
        };
        let (ids, at): (Vec<EntryId>, Option<usize>) = {
            let ws = self.app.workspace.read();
            let Some(pane) = ws.pane(self.pane) else {
                return;
            };
            let own: Vec<(usize, EntryId)> = group
                .iter()
                .enumerate()
                .filter(|(_, id)| pane.depth(**id) == 0)
                .map(|(at, id)| (span.start + at, *id))
                .collect();
            let at = own.last().map(|(at, _)| *at);
            (own.into_iter().map(|(_, id)| id).collect(), at)
        };
        let args = SelectArgs {
            ids,
            mode: if add {
                SelectMode::Add
            } else {
                SelectMode::Replace
            },
            pane: Some(self.pane),
            at,
            ..SelectArgs::default()
        };
        self.run_args("select", args, window, cx);
    }

    /// Put the keys on group `group`'s heading (ADR-131, 1): the cursor
    /// stays where it was, its ring on the heading.
    pub fn stop_on_heading(&mut self, group: usize, cx: &mut Context<Self>) {
        let cursor = self
            .app
            .workspace
            .read()
            .pane(self.pane)
            .and_then(|p| p.selection.cursor);
        let list = &mut self.ui_mut().list;
        let Some(span) = list.rows.spans().get(group) else {
            return;
        };
        list.heading = Some(HeadingStop {
            group,
            of: span.group.clone(),
            cursor,
        });
        cx.notify();
    }

    /// The group whose heading the keys are on.
    pub fn heading_stop(&self) -> Option<usize> {
        self.ui().list.heading.as_ref().map(|stop| stop.group)
    }

    /// After a command run from a heading (its group selected, folded), the
    /// keys stay on it, wherever the cursor went.
    fn keep_on_heading(&mut self) {
        let cursor = self
            .app
            .workspace
            .read()
            .pane(self.pane)
            .and_then(|p| p.selection.cursor);
        if let Some(stop) = &mut self.ui_mut().list.heading {
            stop.cursor = cursor;
        }
    }

    /// Fold or open (`mode`) the group whose heading the keys are on,
    /// which they stay on.
    pub fn fold_from_heading(
        &mut self,
        group: usize,
        mode: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let pane = self.pane;
        self.run(
            "group_fold",
            serde_json::json!({ "group": group, "mode": mode, "pane": pane }),
            window,
            cx,
        );
        self.keep_on_heading();
        cx.notify();
    }

    /// What a key bound to a command does while the keys are on a heading
    /// (ADR-131, 1): Enter and Space select its group, the Menu key and
    /// Shift+F10 open its menu. Whether it took the key.
    pub fn heading_command(
        &mut self,
        name: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        let Some(group) = self.heading_stop() else {
            return false;
        };
        match name {
            "open" | "ui.peek" => {
                self.select_group(Some(group), false, window, cx);
                self.keep_on_heading();
                cx.notify();
                true
            }
            "ui.context_menu" => {
                self.open_group_menu(group, gpui::Point::default(), window, cx);
                let pane = self.pane;
                if let Some(menu) = &mut self.menu {
                    menu.highlighted = Some(0);
                    menu.keyboard = Some(crate::menu::KeyboardAnchor::new(
                        crate::menu::MenuHost::Pane(pane),
                    ));
                }
                true
            }
            _ => false,
        }
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
        let handled = view.nav(nav, step, self, window, cx);
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
        // A press on an entry takes the keys off a heading.
        self.with_pane(pane, |this| this.ui_mut().list.heading = None);
        // Gone since it was drawn: the press is on nothing. An entry a
        // view lent (ADR-116) is in no order: the pane says whether it is
        // still shown.
        if self.ui().list.index_of(id).is_none() && !self.is_lent(id) {
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
            .h(sized(28.))
            .max_w_full()
            .px(sized(12.))
            .flex()
            .items_center()
            .gap(sized(8.))
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
                    .child(div().flex_none().w(px(2.)).h(sized(16.)).bg(theme.accent)),
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
                .px(sized(16.))
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
                .filter(|p| !p.sort.needs_metadata())
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
                let covered = shell.band_covers(&covered);
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

    /// What a band covering the rows at `covered` takes of them, in the
    /// order shown (`Pane::covered`).
    fn band_covers(&self, covered: &[usize]) -> Vec<EntryId> {
        let sorted = &self.ui().list.sorted;
        let rows = covered
            .iter()
            .filter_map(|index| sorted.get(*index).copied());
        let ws = self.app.workspace.read();
        ws.pane(self.pane)
            .map(|p| p.covered(rows, None))
            .unwrap_or_default()
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
                let mut ids = drag.kept;
                ids.extend(shell.band_covers(&drag.covered));
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
        let ground = cx.global::<Theme>().files_ground(self.lowered());
        // Its ground under a drop's tint, which takes the place of the
        // ground while a drag is over it.
        let tint = ground.blend(cx.global::<Theme>().drop_target);
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
            // The files' area: the window's own ground (`window_bg`).
            .bg(ground)
            .track_focus(&self.ui().focus)
            .key_context(key_context)
            .children(self.menu_area_marker(pane))
            .on_key_down(cx.listener(Shell::on_pane_key_down))
            // On a heading, Enter, Space and the Menu key are the
            // heading's (ADR-131, 1).
            .capture_action(
                cx.listener(move |this, action: &keymap::RunCommand, window, cx| {
                    if this.with_pane(pane, |this| this.heading_command(&action.name, window, cx)) {
                        cx.stop_propagation();
                    }
                }),
            )
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
