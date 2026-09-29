//! UI-facing registries: views, panels, places, menu items and the sources
//! of those made when a menu opens, key bindings and key schemes, and what keeps a
//! location in the sidebar.
//! The core stores descriptors tagged by owner; the UI crate interprets them.
//! Every contribution can be withdrawn by owner when a plugin is disabled.

use std::any::Any;
use std::collections::HashSet;
use std::sync::Arc;
use std::sync::atomic::AtomicU64;

use parking_lot::RwLock;
use serde::{Deserialize, Serialize};

use crate::command::Invocation;
use crate::location::Location;

/// Owners whose contributions are refused: disabled plugins. Shared by
/// every list of a [`Registry`], so a plugin's worker that publishes after
/// the plugin was disabled cannot bring its items back.
#[derive(Default)]
pub struct OwnerGate {
    closed: RwLock<HashSet<Arc<str>>>,
}

impl OwnerGate {
    pub fn set_open(&self, owner: &str, open: bool) {
        let mut closed = self.closed.write();
        if open {
            closed.remove(owner);
        } else {
            closed.insert(owner.into());
        }
    }

    pub fn is_open(&self, owner: &str) -> bool {
        !self.closed.read().contains(owner)
    }
}

/// A list of contributions, each tagged with its owner (plugin id or `core`).
pub struct Contributions<T> {
    items: RwLock<Vec<(Arc<str>, T)>>,
    /// Bumped on every add or removal, so per-frame consumers compare one
    /// atomic instead of taking the read lock and walking the items.
    revision: AtomicU64,
    gate: Arc<OwnerGate>,
}

impl<T> Default for Contributions<T> {
    fn default() -> Self {
        Self::gated(Arc::default())
    }
}

impl<T> Contributions<T> {
    fn gated(gate: Arc<OwnerGate>) -> Self {
        Contributions {
            items: RwLock::new(Vec::new()),
            revision: AtomicU64::new(0),
            gate,
        }
    }

    /// Changes whenever the set of contributions changes (including a
    /// remove followed by an add of the same id).
    pub fn revision(&self) -> u64 {
        self.revision.load(std::sync::atomic::Ordering::Acquire)
    }

    fn bump(&self) {
        self.revision
            .fetch_add(1, std::sync::atomic::Ordering::AcqRel);
    }
}

impl<T: Clone> Contributions<T> {
    /// Add an item. Refused (returns false) while `owner` is disabled.
    pub fn add(&self, owner: &str, item: T) -> bool {
        let mut items = self.items.write();
        if !self.gate.is_open(owner) {
            return false;
        }
        items.push((owner.into(), item));
        drop(items);
        self.bump();
        true
    }

    /// Replace everything `owner` contributed with `items`, in one step:
    /// a reader sees the old list or the new one, never a partial one, and
    /// the revision moves once. Refused (returns false) while `owner` is
    /// disabled.
    pub fn replace_owner(&self, owner: &str, new: impl IntoIterator<Item = T>) -> bool {
        let owner: Arc<str> = owner.into();
        let mut items = self.items.write();
        if !self.gate.is_open(&owner) {
            return false;
        }
        items.retain(|(o, _)| *o != owner);
        items.extend(new.into_iter().map(|t| (Arc::clone(&owner), t)));
        drop(items);
        self.bump();
        true
    }

    pub fn remove_owner(&self, owner: &str) {
        let mut items = self.items.write();
        let before = items.len();
        items.retain(|(o, _)| &**o != owner);
        if items.len() != before {
            drop(items);
            self.bump();
        }
    }

    pub fn all(&self) -> Vec<T> {
        self.items.read().iter().map(|(_, t)| t.clone()).collect()
    }

    pub fn find(&self, pred: impl Fn(&T) -> bool) -> Option<T> {
        self.items
            .read()
            .iter()
            .map(|(_, t)| t)
            .find(|t| pred(t))
            .cloned()
    }
}

/// An opaque, toolkit-specific factory a plugin hands to the UI crate.
pub type Factory = Arc<dyn Any + Send + Sync>;

/// A way to render a listing (list, grid, columns, ...).
#[derive(Clone)]
pub struct ViewSpec {
    pub id: Arc<str>,
    pub title: String,
    /// Bundled symbolic toolbar icon; unknown icons use the generic view icon.
    pub icon: String,
    /// Display order in the view selector. Bundled base views precede plugin views.
    pub order: i32,
    pub factory: Factory,
    /// Producers increment this before waking the app, avoiding per-frame data queries.
    pub revision: Arc<AtomicU64>,
    pub role: ViewRole,
}

/// What a view is for, which decides where it is offered. Every view is in
/// the View menu, the palette and the keymap; the toolbar's view buttons
/// hold the [`ViewRole::Listing`] views, and any other only while it is
/// shown, so the pane says what it is in and one click leads back.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ViewRole {
    /// The folder's entries in a shape of their own (list, grid, columns):
    /// switched between all the time.
    #[default]
    Listing,
    /// The folder's entries in a shape chosen now and then (the tree):
    /// offered like a lens, so the toolbar keeps the shapes used all the
    /// time.
    Alternate,
    /// A question about the folder answered with data the view gathers
    /// itself (Usage's sizes): chosen now and then.
    Lens,
}

/// A renderer supplied by the UI, selected without a toolkit dependency in the plugin.
#[derive(Clone)]
pub struct ViewRenderer(pub &'static str);

impl ViewSpec {
    pub fn named(id: &'static str, title: &str, icon: &str, revision: Arc<AtomicU64>) -> Self {
        Self {
            id: id.into(),
            title: title.into(),
            icon: icon.into(),
            order: 100,
            factory: Arc::new(ViewRenderer(id)),
            revision,
            role: ViewRole::Listing,
        }
    }

    /// A view offered as a [`ViewRole::Lens`].
    #[must_use]
    pub fn lens(mut self) -> Self {
        self.role = ViewRole::Lens;
        self
    }

    /// A view offered as a [`ViewRole::Alternate`].
    #[must_use]
    pub fn alternate(mut self) -> Self {
        self.role = ViewRole::Alternate;
        self
    }
}

/// A dockable panel (preview, shelf, properties, ...).
#[derive(Clone)]
pub struct PanelSpec {
    pub id: Arc<str>,
    pub title: String,
    pub factory: Factory,
}

/// A sidebar entry.
#[derive(Clone, Debug, Serialize)]
pub struct Place {
    pub title: String,
    pub location: Location,
    pub icon: Option<String>,
    /// Sidebar section, e.g. `places`, `bookmarks`, `searches`, `devices`.
    pub group: String,
    /// Free and total bytes, for volumes; filled in after a worker asked.
    pub space: Option<(u64, u64)>,
    /// A command that detaches the volume (the sidebar shows an eject button).
    pub eject: Option<Invocation>,
    /// Run this instead of opening `location`: a volume that must be mounted
    /// (or unlocked) first.
    pub activate: Option<Invocation>,
    /// Why the place cannot be opened as it is (a volume to mount, a
    /// bookmarked folder that is not there): the row is drawn as not
    /// available, with this as its tooltip.
    pub unavailable: Option<String>,
    /// What its owner is doing to it now, in its words ("Unlocking…",
    /// "Unmounting…"): the row says so in place of its details, and is
    /// not activated or ejected again until the owner publishes it without.
    pub busy: Option<String>,
    /// The place's own items in its context menu (rename or remove a
    /// bookmark), with their arguments; the UI adds Open and Eject. `when`
    /// is not evaluated: the owner lists what applies to this place.
    pub menu: Vec<MenuItem>,
    /// The command that takes this place out of the sidebar (removes the
    /// bookmark, forgets the saved search), for a place the user kept: the
    /// pane's keep button runs it while the pane shows `location`.
    pub forget: Option<Invocation>,
    /// How the place is put in order among its group by dragging.
    pub reorder: Option<Reorder>,
}

/// How a place moves among the places of its group that have one: `command`
/// moves this place (its arguments name it), with `"before"` set to the
/// `before` of the place it goes in front of, or left out for last.
#[derive(Clone, Debug, Serialize)]
pub struct Reorder {
    pub command: Invocation,
    /// This place as the `before` argument of the group's move command.
    pub before: serde_json::Value,
}

/// What keeps locations of one scheme in the sidebar: folders as
/// bookmarks, searches as saved searches. The core `keep` command runs
/// `save` for the location a pane shows, adding the pane's id; each place
/// it kept carries its own [`Place::forget`]. The words are the pane
/// button's.
#[derive(Clone, Debug, Serialize)]
pub struct Keeper {
    /// The [`Location::scheme`] it keeps (`path`, `query`).
    pub scheme: String,
    pub save: Invocation,
    /// "Bookmark this folder".
    pub save_title: String,
    /// "Remove from bookmarks".
    pub forget_title: String,
}

/// A menu contribution. `when` is a space-separated list of tokens that must
/// all hold (`selection`, `single`, `dir`, `file`, `empty` for the menu
/// opened on the background, and `folder` while the pane shows a folder
/// rather than a search or the shelf); `None` means always. An item about
/// the location rather than about files (Refresh, a view) says `empty`, so
/// the menu of a file stays about that file; one that needs the folder
/// itself (New Folder, Bookmark This Folder) says `empty folder`. A token
/// of alternatives, `selection|folder`, holds when either does.
///
/// Two tokens say what the selected entries are, as their names tell it
/// (no file is read): `kind:image` holds when every one is of that kind,
/// `has:image` when one at least is. The part after the colon is a kind
/// (`image`, `video`, `audio`, `document`, `archive`, `text`, … as the
/// desktop groups types, the query's `kind:`), a MIME type matched with
/// its parents (`text/plain` holds for source code; `image/*`), or an
/// extension (`.gpx`). A file whose name says nothing is of no kind: it
/// fails `kind:` and does not count for `has:`. These two are for menus:
/// a key binding's `when` is the pane's key context, which has no types.
///
/// `submenu` moves a rarely used item under a parent row of that name,
/// after the other groups. `icon` names a bundled symbolic icon.
///
/// `order` sorts items and groups them: the hundreds digit is the group and
/// the UI draws a separator between groups. Convention for `context`:
/// 0xx open/navigate, 1xx edit (rename, cut, copy, paste), 2xx create,
/// 3xx destructive (trash, delete), 4xx misc (refresh).
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct MenuItem {
    /// `context`, `main`, or a plugin-defined menu id.
    pub menu: String,
    pub title: String,
    pub command: Invocation,
    pub when: Option<String>,
    pub order: i32,
    pub icon: Option<String>,
    pub submenu: Option<String>,
}

/// Items of a menu that depend on what the owner knows when it opens
/// ("Open With Image Viewer"): the UI dispatches `command` with the pane
/// (`{"pane": id}`) each time the menu opens where `when` holds (the same
/// tokens as [`MenuItem::when`]), and adds the [`MenuItem`]s it answers
/// (`{"items": [...]}`), whose own `when` is not evaluated.
///
/// The command runs on the UI thread while the menu opens: it answers from
/// what the owner keeps in memory, and refreshes that on a worker. An
/// answer of no items, or an error, leaves the menu as it would be without
/// the source; one with items takes the place of the static items whose
/// command is in `replaces`, which stay registered as the fallback and for
/// the palette.
#[derive(Clone, Debug, Serialize)]
pub struct MenuSource {
    /// `context`, like [`MenuItem::menu`].
    pub menu: String,
    pub when: Option<String>,
    pub command: Invocation,
    pub replaces: Vec<Invocation>,
}

/// A key binding. `keys` uses the `ctrl-shift-n` notation; chords are
/// space-separated. Plugins add their defaults to `Registry::keymap`, below
/// the shell's own keys; a key scheme adds its keys to `Registry::schemes`,
/// over the shell's (ADR-024, and `crate::keymap` for the user's).
#[derive(Clone, Debug, Serialize)]
pub struct KeyBinding {
    pub keys: String,
    pub command: Invocation,
    pub when: Option<String>,
}

/// All UI registries.
pub struct Registry {
    pub views: Contributions<ViewSpec>,
    pub panels: Contributions<PanelSpec>,
    pub places: Contributions<Place>,
    pub menus: Contributions<MenuItem>,
    pub menu_sources: Contributions<MenuSource>,
    pub keymap: Contributions<KeyBinding>,
    /// A key scheme's bindings (the `vim` plugin's): over the shell's own
    /// and every plugin's defaults, whatever order plugins were enabled in.
    pub schemes: Contributions<KeyBinding>,
    pub keepers: Contributions<Keeper>,
    gate: Arc<OwnerGate>,
}

impl Default for Registry {
    fn default() -> Self {
        let gate = Arc::new(OwnerGate::default());
        Registry {
            views: Contributions::gated(Arc::clone(&gate)),
            panels: Contributions::gated(Arc::clone(&gate)),
            places: Contributions::gated(Arc::clone(&gate)),
            menus: Contributions::gated(Arc::clone(&gate)),
            menu_sources: Contributions::gated(Arc::clone(&gate)),
            keymap: Contributions::gated(Arc::clone(&gate)),
            schemes: Contributions::gated(Arc::clone(&gate)),
            keepers: Contributions::gated(Arc::clone(&gate)),
            gate,
        }
    }
}

impl Registry {
    /// Accept or refuse contributions from `owner`. The plugin host closes
    /// the gate before withdrawing a disabled plugin's items and opens it
    /// before registering an enabled one.
    pub fn set_owner_enabled(&self, owner: &str, enabled: bool) {
        self.gate.set_open(owner, enabled);
    }

    pub fn remove_owner(&self, owner: &str) {
        self.views.remove_owner(owner);
        self.panels.remove_owner(owner);
        self.places.remove_owner(owner);
        self.menus.remove_owner(owner);
        self.menu_sources.remove_owner(owner);
        self.keymap.remove_owner(owner);
        self.schemes.remove_owner(owner);
        self.keepers.remove_owner(owner);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn revision_changes_on_add_and_effective_removal_only() {
        let c = Contributions::<u32>::default();
        let r0 = c.revision();
        c.add("a", 1);
        let r1 = c.revision();
        assert_ne!(r0, r1);
        c.remove_owner("nobody");
        assert_eq!(c.revision(), r1);
        c.remove_owner("a");
        assert_ne!(c.revision(), r1);
    }

    #[test]
    fn replacing_is_one_step_and_a_closed_owner_is_refused() {
        let registry = Registry::default();
        let c = &registry.keymap;
        let binding = |keys: &str| KeyBinding {
            keys: keys.into(),
            command: Invocation::new("x"),
            when: None,
        };
        c.add("p", binding("a"));
        c.add("q", binding("b"));
        let r = c.revision();
        assert!(c.replace_owner("p", [binding("c"), binding("d")]));
        assert_eq!(c.revision(), r + 1);
        let keys: Vec<String> = c.all().into_iter().map(|b| b.keys).collect();
        assert_eq!(keys, ["b", "c", "d"]);

        registry.set_owner_enabled("p", false);
        registry.remove_owner("p");
        assert!(!c.add("p", binding("late")));
        assert!(!c.replace_owner("p", [binding("late")]));
        assert_eq!(c.all().len(), 1);
        registry.set_owner_enabled("p", true);
        assert!(c.add("p", binding("again")));
    }
}
