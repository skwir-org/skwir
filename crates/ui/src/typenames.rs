//! Human names of file types ("PDF document") for the Type column and the
//! preview header, and their kinds ("Image") for the Kind column. The names
//! come from the MIME database's per-type files, so they are resolved once
//! per type on a worker, with the type's kind; until the answer arrives a
//! row shows a name derived from its extension, and no kind.

use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex};

use skwir_core::App;
use skwir_platform::mime::Mime;

/// A type, its name, and what its kind is called: `Other` for none.
type Resolved = Vec<(String, Option<String>, &'static str)>;

#[derive(Default)]
pub struct TypeNames {
    known: HashMap<String, Option<Arc<str>>>,
    kinds: HashMap<String, Arc<str>>,
    wanted: HashSet<String>,
    results: Arc<Mutex<Resolved>>,
}

/// What one file of `kind` is: `Image`, `Audio`, `Text` (a group of them
/// is called by `Kind::title`).
fn one_of(kind: skwir_platform::mime::Kind) -> &'static str {
    use skwir_platform::mime::Kind;
    match kind {
        Kind::Folder => "Folder",
        Kind::Image => "Image",
        Kind::Video => "Video",
        Kind::Audio => "Audio",
        Kind::Document => "Document",
        Kind::Spreadsheet => "Spreadsheet",
        Kind::Presentation => "Presentation",
        Kind::Text => "Text",
        Kind::Archive => "Archive",
        Kind::Font => "Font",
        Kind::Program => "Program",
    }
}

/// The type of a link that leads nowhere (a link that resolves is named
/// as what it leads to).
pub const BROKEN_LINK: &str = "Broken link";

/// What an entry is called until the database's name for its type
/// arrives: a pipe or a device by its kind (never by its name, which says
/// nothing of it), a file by its extension (`fallback`).
pub fn fallback_of(entry: &skwir_core::Entry) -> String {
    use skwir_core::EntryKind;
    match entry.opens() {
        EntryKind::Fifo => "Pipe".to_owned(),
        EntryKind::Socket => "Socket".to_owned(),
        EntryKind::CharDevice => "Character device".to_owned(),
        EntryKind::BlockDevice => "Block device".to_owned(),
        _ => fallback(&entry.type_name().to_string_lossy()),
    }
}

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

    /// What a file of `mime` is (`Image`, `Other`) once its type was read;
    /// otherwise note that it is wanted.
    pub fn kind(&mut self, mime: &Mime) -> Option<Arc<str>> {
        if let Some(kind) = self.kinds.get(mime.as_str()) {
            return Some(Arc::clone(kind));
        }
        if !self.known.contains_key(mime.as_str()) {
            self.wanted.insert(mime.as_str().to_owned());
        }
        None
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
            skwir_core::scheduler::Priority::Visible,
            "type-names",
            app.lifetime().child(),
            move || {
                let resolved: Resolved = types
                    .into_iter()
                    .map(|mime| {
                        let one = Mime(mime.clone());
                        let name = one.description();
                        // Its first kind, as a sort by kind finds it.
                        let kind = skwir_platform::mime::kinds(&one)
                            .into_iter()
                            .next()
                            .map_or("Other", one_of);
                        (mime, name, kind)
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
        let changed = !found.is_empty();
        for (mime, name, kind) in found {
            self.kinds.insert(mime.clone(), Arc::from(kind));
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
