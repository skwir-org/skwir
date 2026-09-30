//! Human names of file types ("PDF document") for the Type column and the
//! preview header. The names come from the MIME database's per-type files,
//! so they are resolved once per type on a worker; until the answer arrives a
//! row shows a name derived from its extension.

use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex};

use bibli_core::App;
use bibli_platform::mime::Mime;

type Resolved = Vec<(String, Option<String>)>;

#[derive(Default)]
pub struct TypeNames {
    known: HashMap<String, Option<Arc<str>>>,
    wanted: HashSet<String>,
    results: Arc<Mutex<Resolved>>,
}

/// The type of a link that leads nowhere (a link that resolves is named
/// as what it leads to).
pub const BROKEN_LINK: &str = "Broken link";

/// `CSV file` for `budget.csv`, `File` without an extension.
pub fn fallback(name: &str) -> String {
    match name.rsplit_once('.') {
        Some((stem, extension))
            if !stem.is_empty()
                && !extension.is_empty()
                && extension.len() <= 5
                && extension.chars().all(char::is_alphanumeric) =>
        {
            format!("{} file", extension.to_uppercase())
        }
        _ => "File".to_owned(),
    }
}

impl TypeNames {
    /// The database's name if it is known; otherwise note that it is wanted.
    pub fn get(&mut self, mime: &Mime) -> Option<Arc<str>> {
        if let Some(name) = self.known.get(mime.as_str()) {
            return name.clone();
        }
        self.wanted.insert(mime.as_str().to_owned());
        None
    }

    /// Read-only lookup for render paths that cannot request.
    pub fn peek(&self, mime: &str) -> Option<Arc<str>> {
        self.known.get(mime).cloned().flatten()
    }

    /// Resolve what `get` missed, in one job. Call once after building rows.
    pub fn request(&mut self, app: &Arc<App>) {
        if self.wanted.is_empty() {
            return;
        }
        let types: Vec<String> = self.wanted.drain().collect();
        // Pending types read as unknown, so they are not requested again.
        for mime in &types {
            self.known.insert(mime.clone(), None);
        }
        let results = Arc::clone(&self.results);
        let weak = app.weak();
        app.scheduler.spawn(
            bibli_core::scheduler::Priority::Visible,
            "type-names",
            app.lifetime().child(),
            move || {
                let resolved: Resolved = types
                    .into_iter()
                    .map(|mime| {
                        let name = Mime(mime.clone()).description();
                        (mime, name)
                    })
                    .collect();
                if let Ok(mut slot) = results.lock() {
                    slot.extend(resolved);
                }
                if let Some(app) = weak.upgrade() {
                    app.wake();
                }
            },
        );
    }

    /// Take finished lookups. Returns whether anything changed.
    pub fn absorb(&mut self) -> bool {
        let Ok(mut slot) = self.results.lock() else {
            return false;
        };
        let found = std::mem::take(&mut *slot);
        drop(slot);
        let changed = found.iter().any(|(_, name)| name.is_some());
        for (mime, name) in found {
            self.known.insert(mime, name.map(Arc::from));
        }
        changed
    }
}

#[cfg(test)]
mod tests {
    use super::fallback;

    #[test]
    fn fallback_names_come_from_the_extension() {
        assert_eq!(fallback("budget 2026.csv"), "CSV file");
        assert_eq!(fallback("Makefile"), "File");
        assert_eq!(fallback(".hidden"), "File");
        assert_eq!(fallback("notes.backup-of-march"), "File");
    }
}
