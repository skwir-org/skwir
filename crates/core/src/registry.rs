//! UI-facing registries: views, panels, places, menu items, key bindings.
//! The core stores descriptors tagged by owner; the UI crate interprets them.
//! Every contribution can be withdrawn by owner when a plugin is disabled.

use std::any::Any;
use std::sync::Arc;

use parking_lot::RwLock;
use serde::Serialize;

use crate::command::Invocation;
use crate::location::Location;

/// A list of contributions, each tagged with its owner (plugin id or `core`).
pub struct Contributions<T> {
    items: RwLock<Vec<(Arc<str>, T)>>,
}

impl<T> Default for Contributions<T> {
    fn default() -> Self {
        Contributions {
            items: RwLock::new(Vec::new()),
        }
    }
}

impl<T: Clone> Contributions<T> {
    pub fn add(&self, owner: &str, item: T) {
        self.items.write().push((owner.into(), item));
    }

    pub fn remove_owner(&self, owner: &str) {
        self.items.write().retain(|(o, _)| &**o != owner);
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
    pub factory: Factory,
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
    /// Sidebar section, e.g. `places`, `devices`, `bookmarks`.
    pub group: String,
}

/// A menu contribution. `when` is an expression the UI evaluates
/// (e.g. `selection.count == 1`); `None` means always.
#[derive(Clone, Debug, Serialize)]
pub struct MenuItem {
    /// `context`, `main`, or a plugin-defined menu id.
    pub menu: String,
    pub title: String,
    pub command: Invocation,
    pub when: Option<String>,
    pub order: i32,
}

/// A key binding. `keys` uses the `ctrl-shift-n` notation; chords are space-separated.
#[derive(Clone, Debug, Serialize)]
pub struct KeyBinding {
    pub keys: String,
    pub command: Invocation,
    pub when: Option<String>,
}

/// All UI registries.
#[derive(Default)]
pub struct Registry {
    pub views: Contributions<ViewSpec>,
    pub panels: Contributions<PanelSpec>,
    pub places: Contributions<Place>,
    pub menus: Contributions<MenuItem>,
    pub keymap: Contributions<KeyBinding>,
}

impl Registry {
    pub fn remove_owner(&self, owner: &str) {
        self.views.remove_owner(owner);
        self.panels.remove_owner(owner);
        self.places.remove_owner(owner);
        self.menus.remove_owner(owner);
        self.keymap.remove_owner(owner);
    }
}
