//! Metadata providers: plugins that fill entry fields during hydration
//! (tags from xattrs, MIME types, media dimensions, ...). Called in batches
//! on a worker right after stat, never on the UI thread.

use std::path::Path;
use std::sync::Arc;

use parking_lot::RwLock;

use crate::entry::Entry;

/// Shared on-disk tag format used by the tags provider and copy operations.
pub const TAGS_XATTR: &str = "user.xdg.tags";

/// Fills fields on freshly stat-ed entries.
pub trait MetadataProvider: Send + Sync {
    fn id(&self) -> &'static str;
    /// Fill what you know for each `(path, entry)`. Set fields with your own
    /// provenance (`Provenance::Plugin { name }`) and namespaced keys.
    fn fill(&self, dir: &Path, entries: &mut [Entry]);
}

/// The registered providers, in registration order.
#[derive(Default)]
pub struct MetadataProviders {
    list: RwLock<Vec<(Arc<str>, Arc<dyn MetadataProvider>)>>,
}

impl MetadataProviders {
    pub fn register(&self, owner: &str, provider: Arc<dyn MetadataProvider>) {
        self.list.write().push((owner.into(), provider));
    }

    pub fn unregister_owner(&self, owner: &str) {
        self.list.write().retain(|(o, _)| &**o != owner);
    }

    pub fn ids(&self) -> Vec<&'static str> {
        self.list.read().iter().map(|(_, p)| p.id()).collect()
    }

    /// Run every provider over `entries` (all children of `dir`).
    pub fn fill(&self, dir: &Path, entries: &mut [Entry]) {
        if entries.is_empty() {
            return;
        }
        let providers: Vec<Arc<dyn MetadataProvider>> = self
            .list
            .read()
            .iter()
            .map(|(_, p)| Arc::clone(p))
            .collect();
        for p in providers {
            p.fill(dir, entries);
        }
    }
}
