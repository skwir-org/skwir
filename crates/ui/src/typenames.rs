//! Human names of file types ("PDF document") for the Type column and the
//! preview header, and their kinds ("Image") for the Kind column. The names
//! come from the MIME database's per-type files, so they are resolved once
//! per type on a worker, with the type's kind; until the answer arrives a
//! row shows a name derived from its extension, and no kind.

use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex};

use skwir_core::App;
use skwir_core::words;
use skwir_core::words::Words;
use skwir_platform::mime::{Kind, Mime};

/// A type, its name, and its kind: `None` for none.
type Resolved = Vec<(String, Option<String>, Option<Kind>)>;

#[derive(Default)]
pub struct TypeNames {
    known: HashMap<String, Option<Arc<str>>>,
    /// Each type's kind, as data: said where it is drawn.
    kinds: HashMap<String, Option<Kind>>,
    wanted: HashSet<String>,
    results: Arc<Mutex<Resolved>>,
}

/// What one file of `kind` is: `Image`, `Audio`, `Text` (a group of them
/// is called by `Kind::words`); `Other` for none.
fn one_of(kind: Option<Kind>) -> Words {
    let Some(kind) = kind else {
        return words!(
            typenames_kind_other = "Other",
            note = "What one file is, in the Kind column and caption, where its type is of no kind named."
        );
    };
    match kind {
        Kind::Folder => words!(
            typenames_kind_folder = "Folder",
            note = "What one file is, in the Kind column and caption: a folder."
        ),
        Kind::Image => words!(
            typenames_kind_image = "Image",
            note = "What one file is, in the Kind column and caption: a picture."
        ),
        Kind::Video => words!(
            typenames_kind_video = "Video",
            note = "What one file is, in the Kind column and caption: a film."
        ),
        Kind::Audio => words!(
            typenames_kind_audio = "Audio",
            note = "What one file is, in the Kind column and caption: a sound or music."
        ),
        Kind::Document => words!(
            typenames_kind_document = "Document",
            note = "What one file is, in the Kind column and caption: a document."
        ),
        Kind::Spreadsheet => words!(
            typenames_kind_spreadsheet = "Spreadsheet",
            note = "What one file is, in the Kind column and caption: a spreadsheet."
        ),
        Kind::Presentation => words!(
            typenames_kind_presentation = "Presentation",
            note = "What one file is, in the Kind column and caption: a presentation's slides."
        ),
        Kind::Text => words!(
            typenames_kind_text = "Text",
            note = "What one file is, in the Kind column and caption: a text."
        ),
        Kind::Archive => words!(
            typenames_kind_archive = "Archive",
            note = "What one file is, in the Kind column and caption: an archive: files packed in one."
        ),
        Kind::Font => words!(
            typenames_kind_font = "Font",
            note = "What one file is, in the Kind column and caption: a font."
        ),
        Kind::Program => words!(
            typenames_kind_program = "Program",
            note = "What one file is, in the Kind column and caption: a program."
        ),
    }
}

/// What a folder's type is called, in the Type and Kind columns.
pub fn folder() -> Words {
    words!(
        typenames_folder = "Folder",
        note = "The type of a folder, in the Type and Kind columns and captions."
    )
}

/// The type of a link that leads nowhere (a link that resolves is named
/// as what it leads to).
pub fn broken_link() -> Words {
    words!(
        typenames_broken_link = "Broken link",
        note = "The type of a link whose target is gone, in the Type column and the preview."
    )
}

/// What an entry is called until the database's name for its type
/// arrives: a pipe or a device by its kind (never by its name, which says
/// nothing of it), a file by its extension (`fallback`).
pub fn fallback_of(entry: &skwir_core::Entry) -> String {
    use skwir_core::EntryKind;
    match entry.opens() {
        EntryKind::Fifo => words!(
            typenames_pipe = "Pipe",
            note = "The type of a named pipe (a FIFO), in the Type column and the preview."
        )
        .say(),
        EntryKind::Socket => words!(
            typenames_socket = "Socket",
            note = "The type of a socket file, in the Type column and the preview."
        )
        .say(),
        EntryKind::CharDevice => words!(
            typenames_char_device = "Character device",
            note = "The type of a device file read a character at a time (a terminal), in the Type column and the preview."
        )
        .say(),
        EntryKind::BlockDevice => words!(
            typenames_block_device = "Block device",
            note = "The type of a device file read in blocks (a disk), in the Type column and the preview."
        )
        .say(),
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
            words!(
                typenames_extension_file = "{ $extension } file",
                extension = extension.to_uppercase(),
                note = "The type of a file the type database names not yet, by its extension: CSV file."
            )
            .say()
        }
        _ => words!(
            typenames_file = "File",
            note = "The type of a file with no extension the type database names not yet."
        )
        .say(),
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
            return Some(Arc::from(one_of(*kind).say()));
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
                        let kind = skwir_platform::mime::kinds(&one).into_iter().next();
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
            self.kinds.insert(mime.clone(), kind);
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
