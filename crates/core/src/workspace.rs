//! Panes, selection, and focus: the state every view and command reads.

use std::sync::Arc;

use indexmap::IndexSet;

use crate::entry::EntryId;
use crate::location::{Listing, Location, OpenLocation};

/// Identifier of a pane within the workspace.
#[derive(
    Clone,
    Copy,
    Debug,
    PartialEq,
    Eq,
    Hash,
    serde::Serialize,
    serde::Deserialize,
    schemars::JsonSchema,
)]
pub struct PaneId(pub u32);

/// Multi-selection with an anchor (for range selection) and a cursor (keyboard focus).
#[derive(Clone, Debug, Default)]
pub struct Selection {
    items: IndexSet<EntryId>,
    pub anchor: Option<EntryId>,
    pub cursor: Option<EntryId>,
}

impl Selection {
    pub fn clear(&mut self) {
        self.items.clear();
        self.anchor = None;
    }

    pub fn set(&mut self, ids: impl IntoIterator<Item = EntryId>) {
        self.items = ids.into_iter().collect();
        self.anchor = self.items.first().copied();
        self.cursor = self.items.last().copied();
    }

    pub fn add(&mut self, id: EntryId) {
        self.items.insert(id);
        self.cursor = Some(id);
        self.anchor.get_or_insert(id);
    }

    pub fn toggle(&mut self, id: EntryId) {
        if !self.items.shift_remove(&id) {
            self.items.insert(id);
            self.anchor.get_or_insert(id);
        }
        self.cursor = Some(id);
    }

    pub fn remove(&mut self, id: EntryId) {
        self.items.shift_remove(&id);
    }

    pub fn contains(&self, id: EntryId) -> bool {
        self.items.contains(&id)
    }

    pub fn len(&self) -> usize {
        self.items.len()
    }

    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    pub fn iter(&self) -> impl Iterator<Item = EntryId> + '_ {
        self.items.iter().copied()
    }

    /// Drop ids that no longer exist in the listing.
    pub fn retain_in(&mut self, listing: &Listing) {
        self.items.retain(|id| listing.get(*id).is_some());
        if self.anchor.is_some_and(|a| listing.get(a).is_none()) {
            self.anchor = self.items.first().copied();
        }
        if self.cursor.is_some_and(|c| listing.get(c).is_none()) {
            self.cursor = self.items.last().copied();
        }
    }
}

/// One pane: an open location, its listing, selection, and history.
pub struct Pane {
    pub id: PaneId,
    pub open: Option<Arc<dyn OpenLocation>>,
    pub listing: Listing,
    pub selection: Selection,
    pub history: Vec<Location>,
    /// Index into `history` of the current location.
    pub history_pos: usize,
}

impl Pane {
    fn new(id: PaneId) -> Self {
        Pane {
            id,
            open: None,
            listing: Listing::default(),
            selection: Selection::default(),
            history: Vec::new(),
            history_pos: 0,
        }
    }

    pub fn location(&self) -> Option<&Location> {
        self.open.as_ref().map(|o| o.location())
    }

    /// Apply pending listing events. Returns how many were applied.
    pub fn pump(&mut self) -> usize {
        let Some(open) = &self.open else { return 0 };
        let rx = open.events().clone();
        let n = self.listing.drain(&rx);
        if n > 0 {
            self.selection.retain_in(&self.listing);
        }
        n
    }
}

/// All panes and which one is active.
pub struct Workspace {
    panes: Vec<Pane>,
    active: usize,
    next_id: u32,
}

impl Default for Workspace {
    fn default() -> Self {
        let mut w = Workspace {
            panes: Vec::new(),
            active: 0,
            next_id: 0,
        };
        w.add_pane();
        w
    }
}

impl Workspace {
    pub fn add_pane(&mut self) -> PaneId {
        let id = PaneId(self.next_id);
        self.next_id += 1;
        self.panes.push(Pane::new(id));
        id
    }

    pub fn close_pane(&mut self, id: PaneId) -> bool {
        if self.panes.len() <= 1 {
            return false;
        }
        let Some(i) = self.panes.iter().position(|p| p.id == id) else {
            return false;
        };
        self.panes.remove(i);
        self.active = self.active.min(self.panes.len() - 1);
        true
    }

    pub fn panes(&self) -> &[Pane] {
        &self.panes
    }

    pub fn panes_mut(&mut self) -> &mut [Pane] {
        &mut self.panes
    }

    pub fn active(&self) -> &Pane {
        &self.panes[self.active]
    }

    pub fn active_mut(&mut self) -> &mut Pane {
        &mut self.panes[self.active]
    }

    pub fn active_id(&self) -> PaneId {
        self.panes[self.active].id
    }

    pub fn set_active(&mut self, id: PaneId) -> bool {
        match self.panes.iter().position(|p| p.id == id) {
            Some(i) => {
                self.active = i;
                true
            }
            None => false,
        }
    }

    pub fn pane(&self, id: PaneId) -> Option<&Pane> {
        self.panes.iter().find(|p| p.id == id)
    }

    pub fn pane_mut(&mut self, id: PaneId) -> Option<&mut Pane> {
        self.panes.iter_mut().find(|p| p.id == id)
    }

    /// Pump every pane. Returns the total number of events applied.
    pub fn pump(&mut self) -> usize {
        self.panes.iter_mut().map(Pane::pump).sum()
    }
}
