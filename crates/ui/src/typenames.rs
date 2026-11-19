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

/// The words' revision types were asked at, and each type, its name, and
/// its kind: `None` for none.
type Resolved = Vec<(u64, String, Option<String>, Option<Kind>)>;

#[derive(Default)]
pub struct TypeNames {
    /// The database's names, read in the language words were said in at
    /// `said_at` (`words::revision`): read again once it moves.
    known: HashMap<String, Option<Arc<str>>>,
    said_at: u64,
    /// Each type's kind, as data: said where it is drawn.
    kinds: HashMap<String, Option<Kind>>,
    /// What one file of each kind is, and what a folder's type is called,
    /// said at `said_at`: once a revision, not once a row.
    kinds_said: HashMap<Option<Kind>, Arc<str>>,
    folder_said: Option<Arc<str>>,
    /// What a file is called while its type's name is not known, by the
    /// extension it is called by (`mime::unnamed_extension`), said at
    /// `said_at`.
    unnamed_said: HashMap<Option<String>, Arc<str>>,
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

/// `CSV file` for `budget.csv`, `File` without an extension, as a type's
/// group names it too (`mime::unnamed`).
pub fn fallback(name: &str) -> String {
    skwir_platform::mime::unnamed(name).say()
}

impl TypeNames {
    /// Forget what was said in words no longer said: the names, read in
    /// the reader's languages, and the kinds' words.
    fn in_words_said(&mut self) {
        let now = skwir_core::words::revision();
        if self.said_at != now {
            self.known.clear();
            self.kinds_said.clear();
            self.folder_said = None;
            self.unnamed_said.clear();
            self.said_at = now;
        }
    }

    /// The database's name if it is known; otherwise note that it is wanted.
    pub fn get(&mut self, mime: &Mime) -> Option<Arc<str>> {
        self.in_words_said();
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
        self.in_words_said();
        if let Some(kind) = self.kinds.get(mime.as_str()).copied() {
            return Some(Arc::clone(
                self.kinds_said
                    .entry(kind)
                    .or_insert_with(|| Arc::from(one_of(kind).say())),
            ));
        }
        if !self.known.contains_key(mime.as_str()) {
            self.wanted.insert(mime.as_str().to_owned());
        }
        None
    }

    /// What `entry` is called until its type's name is known, or where
    /// the database names it not ([`fallback_of`]): a file's said once an
    /// extension while the words stand, not once a row.
    pub fn fallback_of(&mut self, entry: &skwir_core::Entry) -> Arc<str> {
        if entry.opens().is_special() {
            return fallback_of(entry).into();
        }
        self.in_words_said();
        let extension =
            skwir_platform::mime::unnamed_extension(&entry.type_name().to_string_lossy());
        Arc::clone(
            self.unnamed_said
                .entry(extension)
                .or_insert_with_key(|extension| {
                    Arc::from(skwir_platform::mime::unnamed_by(extension.as_deref()).say())
                }),
        )
    }

    /// What a folder's type is called ([`folder`]), said once while the
    /// words stand.
    pub fn folder(&mut self) -> Arc<str> {
        self.in_words_said();
        Arc::clone(
            self.folder_said
                .get_or_insert_with(|| Arc::from(folder().say())),
        )
    }

    /// Resolve what `get` missed, in one job. Call once after building rows.
    pub fn request(&mut self, app: &Arc<App>) {
        if self.wanted.is_empty() {
            return;
        }
        self.in_words_said();
        let types: Vec<String> = self.wanted.drain().collect();
        // Pending types read as unknown, so they are not requested again.
        for mime in &types {
            self.known.insert(mime.clone(), None);
        }
        let results = Arc::clone(&self.results);
        let weak = app.weak();
        // The names are read in the languages of the words said now: an
        // answer asked before they moved is not taken (`absorb`).
        let asked_at = self.said_at;
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
                        (asked_at, mime, name, kind)
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

    /// Take finished lookups. Returns whether anything changed. A name
    /// read in words no longer said is dropped (its type asked again by
    /// the next `get`); its kind, data, is kept.
    pub fn absorb(&mut self) -> bool {
        let Ok(mut slot) = self.results.lock() else {
            return false;
        };
        let found = std::mem::take(&mut *slot);
        drop(slot);
        let changed = !found.is_empty();
        for (asked_at, mime, name, kind) in found {
            self.kinds.insert(mime.clone(), kind);
            if asked_at == self.said_at {
                self.known.insert(mime, name.map(Arc::from));
            }
        }
        changed
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fallback_names_come_from_the_extension() {
        assert_eq!(fallback("budget 2026.csv"), "CSV file");
        assert_eq!(fallback("Makefile"), "File");
        assert_eq!(fallback(".hidden"), "File");
        assert_eq!(fallback("notes.backup-of-march"), "File");
    }

    /// A name read in words no longer said is not taken: its type is
    /// asked again in those said now; its kind, data, is kept.
    #[test]
    fn a_name_asked_before_the_words_moved_is_not_taken() {
        let png = Mime("image/png".into());
        let mut names = TypeNames::default();
        assert_eq!(names.get(&png), None);
        let asked_at = names.said_at;
        // Answered after the language changed and the names were asked
        // anew.
        skwir_core::locale::set_chosen(Some(words::PSEUDO.into()));
        assert_eq!(names.get(&png), None);
        assert!(names.wanted.contains("image/png"), "asked anew");
        names.results.lock().unwrap().push((
            asked_at,
            "image/png".into(),
            Some("PNG image".into()),
            Some(Kind::Image),
        ));
        assert!(names.absorb());
        assert_eq!(names.get(&png), None, "an old answer");
        assert!(names.kind(&png).is_some(), "its kind kept");
        // The answer to what was asked now is taken.
        let now = names.said_at;
        names.results.lock().unwrap().push((
            now,
            "image/png".into(),
            Some("PNG image".into()),
            Some(Kind::Image),
        ));
        names.absorb();
        assert_eq!(names.get(&png).as_deref(), Some("PNG image"));
        skwir_core::locale::set_chosen(None);
    }

    /// What a file of a type not named yet is called is said once an
    /// extension while the words stand, as `fallback` says it.
    #[test]
    fn a_file_named_not_is_said_once_an_extension() {
        use skwir_core::entry::{Entry, EntryId, EntryKind, FileId};
        let file = |ino, name: &str| {
            Entry::new(
                EntryId {
                    file: FileId::new(1, ino),
                    at: 0,
                },
                name,
                EntryKind::File,
            )
        };
        let mut names = TypeNames::default();
        let (a, b) = (file(1, "budget.csv"), file(2, "notes.CSV"));
        let said = names.fallback_of(&a);
        assert_eq!(&*said, fallback("budget.csv"));
        assert!(Arc::ptr_eq(&said, &names.fallback_of(&b)), "one extension");
        assert_eq!(&*names.fallback_of(&file(3, "Makefile")), "File");
        skwir_core::locale::set_chosen(Some(words::PSEUDO.into()));
        let again = names.fallback_of(&a);
        skwir_core::locale::set_chosen(None);
        assert!(!Arc::ptr_eq(&said, &again), "said again");
    }

    /// What a kind and a folder are called is said once while the words
    /// stand, and again once they move.
    #[test]
    fn kinds_are_said_once_a_revision() {
        let png = Mime("image/png".into());
        let mut names = TypeNames::default();
        names.get(&png);
        names.results.lock().unwrap().push((
            names.said_at,
            "image/png".into(),
            None,
            Some(Kind::Image),
        ));
        names.absorb();
        let image = names.kind(&png).unwrap();
        let folder = names.folder();
        assert_eq!(&*image, "Image");
        assert_eq!(&*folder, "Folder");
        assert!(Arc::ptr_eq(&image, &names.kind(&png).unwrap()), "kept");
        assert!(Arc::ptr_eq(&folder, &names.folder()), "kept");
        skwir_core::locale::set_chosen(Some(words::PSEUDO.into()));
        let (image_again, folder_again) = (names.kind(&png).unwrap(), names.folder());
        skwir_core::locale::set_chosen(None);
        assert_ne!(&*image_again, "Image", "said again");
        assert_ne!(&*folder_again, "Folder", "said again");
    }
}
