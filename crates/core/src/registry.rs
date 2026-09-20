//! UI-facing registries: views, panels, places, menu items, key bindings.
//! The core stores descriptors tagged by owner; the UI crate interprets them.
//! Every contribution can be withdrawn by owner when a plugin is disabled.

use std::any::Any;
use std::sync::Arc;
use std::sync::atomic::AtomicU64;

use parking_lot::RwLock;
use serde::Serialize;

use crate::command::Invocation;
use crate::location::Location;

/// A list of contributions, each tagged with its owner (plugin id or `core`).
pub struct Contributions<T> {
    items: RwLock<Vec<(Arc<str>, T)>>,
    /// Bumped on every add or removal, so per-frame consumers compare one
    /// atomic instead of taking the read lock and walking the items.
    revision: AtomicU64,
}

impl<T> Default for Contributions<T> {
    fn default() -> Self {
        Contributions {
            items: RwLock::new(Vec::new()),
            revision: AtomicU64::new(0),
        }
    }
}

impl<T> Contributions<T> {
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
    pub fn add(&self, owner: &str, item: T) {
        self.items.write().push((owner.into(), item));
        self.bump();
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
        }
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
    /// Sidebar section, e.g. `places`, `devices`, `bookmarks`.
    pub group: String,
}

/// A menu contribution. `when` is a space-separated list of tokens that must
/// all hold (`selection`, `single`, `dir`, `file`); `None` means always.
///
/// `order` sorts items and groups them: the hundreds digit is the group and
/// the UI draws a separator between groups. Convention for `context`:
/// 0xx open/navigate, 1xx edit (rename, cut, copy, paste), 2xx create,
/// 3xx destructive (trash, delete), 4xx misc (refresh).
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
}
