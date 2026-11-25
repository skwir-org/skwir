//! What a pane lists, printed (P4; any place, 2026-11-06): its entries as
//! it shows them, in its order, as a table of names, kinds, sizes and
//! times modified, titled by what it is a listing of: a folder by its
//! path, any other place by its name (a search, a tag, the trash,
//! `App::location_title`), with what filters it. The columns are the
//! list view's: where its files come from many folders (a search, a tag,
//! Recent: no folder, nor a place of one folder's files, `Trail::
//! one_folder`), the folder each is in (the trash's, the one it was put
//! there from); where it restores what it lists (`Verbs::restore`), when
//! each was put there. A place listed (a tag) says its kind and how many
//! files it holds, as the list does. Asked of the location's provider,
//! never of its scheme.
//!
//! The entries are copied under the workspace's lock and written out after
//! it, so a folder of 100,000 entries holds the lock only to copy them;
//! those whose metadata the pane has not read yet (entries arrive before
//! it) are read once, a folder at a time, in their remote's lane
//! (`fs::reach::by_lane`): a tag's or Recent's files may be on several
//! servers, one that stopped answering holding its own alone.
use std::collections::HashMap;
use std::path::{Path, PathBuf};

use anyhow::Context as _;
use skwir_core::App;
use skwir_core::cancel::CancelToken;
use skwir_core::entry::{Entry, EntryId, Value, keys};
use skwir_core::location::Location;
use skwir_core::print::Part;
use skwir_core::words;
use skwir_core::workspace::PaneId;

/// A pane's listing, made: what it is a listing of, and its table.
pub(crate) struct Listed {
    pub title: String,
    pub table: Part,
}

/// What a pane shows, copied under the lock: an entry, how many opened
/// folders deep it is shown, and its path.
type Shown = Vec<(Entry, usize, Option<PathBuf>)>;

/// The listing `pane` shows. Reads what the pane has not (each lane's
/// files in a job of that lane, waited for here): never called from a
/// job of a remote's lane, which would wait for itself.
pub(crate) fn listing(app: &App, pane: PaneId, cancel: &CancelToken) -> anyhow::Result<Listed> {
    let (location, filter, mut shown): (Location, Option<String>, Shown) = {
        let ws = app.workspace.read();
        let pane = ws.pane(pane).with_context(crate::no_such_pane)?;
        let location = pane
            .location()
            .cloned()
            .with_context(crate::shows_nothing)?;
        let shown = pane
            .order()
            .iter()
            .filter_map(|&id| Some((pane.entry(id)?.clone(), pane.depth(id), pane.path_of(id))))
            .collect();
        (location, pane.filter().map(str::to_owned), shown)
    };
    let unread: Vec<(EntryId, PathBuf)> = shown
        .iter()
        .filter(|(entry, ..)| !entry.metadata.is_filled() && !entry.is_place())
        .filter_map(|(entry, _, path)| Some((entry.id, path.clone()?)))
        .collect();
    if !unread.is_empty() {
        let mut read: HashMap<EntryId, Entry> = HashMap::new();
        for (lane, items) in skwir_core::fs::reach::by_lane(unread, |(_, path)| path.as_path()) {
            let (weak, token) = (app.weak(), cancel.clone());
            let found = crate::in_lane(app, lane, "print-listing", cancel, move || {
                let app = weak.upgrade().with_context(crate::closing)?;
                let mut found = Vec::new();
                let finished =
                    skwir_core::fs::read_paths(&items, &app.metadata, &token, |entries, _| {
                        found.extend(entries);
                    });
                anyhow::ensure!(finished, crate::cancelled());
                Ok(found)
            })?;
            read.extend(found.into_iter().map(|entry| (entry.id, entry)));
        }
        for (entry, ..) in &mut shown {
            // Its identity and name are the pane's: the rest is read, over
            // what its place said of it (a trash's Deleted), which a read
            // of the file does not say.
            if let Some(found) = read.remove(&entry.id) {
                entry.kind = found.kind;
                entry.target = found.target;
                entry.metadata.merge(found.metadata);
            }
        }
    }
    // What the list's columns say here, asked of the location's provider.
    let one_folder = app.trail(&location).is_some_and(|trail| trail.one_folder);
    let columns = Columns {
        folder: !location.is_path() && !one_folder,
        deleted: app.verbs(&location).restore.is_some() && !one_folder,
    };
    let mut said = Said::new();
    let rows = shown
        .iter()
        .map(|(entry, depth, path)| row(entry, *depth, path.as_deref(), columns, &mut said))
        .collect();
    Ok(Listed {
        title: title(app, &location, filter.as_deref()),
        table: Part::Table {
            columns: columns.names(),
            rows,
        },
    })
}

/// What a listing of `location` is titled: a folder's path, any other
/// place's name (said in the user's language: the page is theirs), then
/// what filters it.
fn title(app: &App, location: &Location, filter: Option<&str>) -> String {
    let named = location.path().map_or_else(
        || app.location_title(location).say_plain(),
        |dir| dir.display().to_string(),
    );
    filtered(named, filter)
}

/// `named` with what filters it, as typed over it (`Pane::filter`), said
/// in the user's language: the listing's title, and its PDF's name.
pub(crate) fn filtered(named: String, filter: Option<&str>) -> String {
    match filter.map(str::trim).filter(|typed| !typed.is_empty()) {
        Some(typed) => words!(
            print_filtered = "{ $named }, filtered by { $filter }",
            named = named,
            filter = typed.to_owned(),
            note = "The title of a listing printed: the folder's path or the place's name, then what filters it, as typed."
        )
        .say_plain(),
        None => named,
    }
}

/// The columns besides a name, a kind, a size and a time modified.
#[derive(Clone, Copy)]
struct Columns {
    /// The folder each file is in.
    folder: bool,
    /// When each file was put in the trash.
    deleted: bool,
}

impl Columns {
    /// Their names, in the list's order: the folder first after the name,
    /// then when it was deleted.
    /// Said here: the table is text.
    fn names(self) -> Vec<String> {
        let mut names = vec![words!(
            print_column_name = "Name",
            note = "A column's header in a printed listing."
        )];
        if self.folder {
            names.push(words!(
                print_column_folder = "Folder",
                note = "A column's header in a printed listing: the folder each file is in."
            ));
        }
        if self.deleted {
            names.push(words!(
                print_column_deleted = "Deleted",
                note = "A column's header in a printed listing of the trash: when each file was put there."
            ));
        }
        names.extend([
            words!(
                print_column_type = "Type",
                note = "A column's header in a printed listing: each file's type (PDF document, Folder…), as the list view's Type column says it."
            ),
            words!(
                print_column_size = "Size",
                note = "A column's header in a printed listing."
            ),
            words!(
                print_column_modified = "Modified",
                note = "A column's header in a printed listing: when each file was last changed."
            ),
        ]);
        names
            .iter()
            .map(skwir_core::words::Words::say_plain)
            .collect()
    }
}

/// What a listing's rows say of many entries alike, read once a listing,
/// never once a row (`AGENTS.md`, "Scale"): the home folder its folders
/// are said from, each type's name (`describe`: `Mime::description`, which
/// reads the type database or asks the registry), and what a file is
/// called while its type's name is not known, by its extension; as the
/// window's `TypeNames` keeps them.
struct Said {
    home: Option<PathBuf>,
    describe: fn(&skwir_platform::mime::Mime) -> Option<String>,
    types: HashMap<skwir_platform::mime::Mime, Option<String>>,
    unnamed: HashMap<Option<String>, String>,
}

impl Said {
    fn new() -> Self {
        Said::describing(skwir_platform::mime::Mime::description)
    }

    /// Each type's name read by `describe`.
    fn describing(describe: fn(&skwir_platform::mime::Mime) -> Option<String>) -> Self {
        Said {
            home: skwir_core::os_path::home(),
            describe,
            types: HashMap::new(),
            unnamed: HashMap::new(),
        }
    }

    /// What `entry`, a file, is, as the window's Type column says it:
    /// its type's name; where the system names it not, by its extension
    /// (`mime::unnamed`), never blank.
    fn type_of(&mut self, entry: &Entry) -> String {
        if let Some(mime) = skwir_platform::mime::of_entry(entry)
            && let Some(name) = self.types.entry(mime).or_insert_with_key(self.describe)
        {
            return name.clone();
        }
        let extension =
            skwir_platform::mime::unnamed_extension(&entry.type_name().to_string_lossy());
        self.unnamed
            .entry(extension)
            .or_insert_with_key(|extension| {
                skwir_platform::mime::unnamed_by(extension.as_deref()).say_plain()
            })
            .clone()
    }
}

/// An entry's row: its name (indented by how deep it is shown, a folder's
/// ending with a slash), the folder it is in and when it was deleted
/// where `columns` say them, its kind, its size and when it was modified,
/// as the list view says them; what is not known yet is left blank.
fn row(
    entry: &Entry,
    depth: usize,
    path: Option<&Path>,
    columns: Columns,
    said: &mut Said,
) -> Vec<String> {
    // A name and a folder as the window draws them: their controls shown,
    // never obeyed (ADR-139, 5), on paper as on screen.
    let mut name = "    ".repeat(depth);
    name.push_str(&skwir_core::words::shown_controls(
        &entry.name.to_string_lossy(),
    ));
    if entry.is_dir() {
        name.push('/');
    }
    let mut row = vec![name];
    if columns.folder {
        // A file of the trash is from the folder it was put there from,
        // which restoring it puts it back in; a place is in none.
        let from = entry
            .metadata
            .value(keys::TRASHED_FROM)
            .and_then(Value::as_str)
            .map(PathBuf::from);
        let folder = from
            .or_else(|| path?.parent().map(Path::to_owned))
            .filter(|_| !entry.is_place());
        row.push(folder.map_or_else(String::new, |dir| {
            skwir_core::os_path::from_home_given(&dir, said.home.as_deref()).controls_shown()
        }));
    }
    if columns.deleted {
        row.push(when(entry.metadata.value(keys::TRASHED)));
    }
    let kind = if entry.is_place() {
        entry
            .metadata
            .value(keys::PLACE_KIND)
            .and_then(Value::as_str)
            .map_or_else(
                || {
                    words!(
                        print_kind_place = "Place",
                        note = "In a printed listing's Kind column: a place listed (a tag) that says no kind of its own."
                    )
                    .say_plain()
                },
                str::to_owned,
            )
    } else if entry.is_dir() {
        words!(
            print_kind_folder = "Folder",
            note = "In a printed listing's Kind column: what a folder is."
        )
        .say_plain()
    } else {
        said.type_of(entry)
    };
    let size = match entry.metadata.value(keys::SIZE).and_then(Value::as_u64) {
        Some(size) if !entry.is_dir() => skwir_platform::inspection::size(size),
        // A place says how many files it holds.
        _ => entry
            .metadata
            .value(keys::FILES)
            .and_then(Value::as_u64)
            .filter(|_| entry.is_place())
            .map(files)
            .unwrap_or_default(),
    };
    row.extend([kind, size, when(entry.metadata.value(keys::MODIFIED))]);
    row
}

/// A time a field holds, to the minute in the system's zone, as the window
/// writes dates (by the locale, or ISO 8601 where the dates setting says
/// so); blank when none is known.
fn when(value: Option<&Value>) -> String {
    value
        .and_then(Value::as_time)
        .and_then(|time| skwir_core::words::Moment::of(time, skwir_core::words::Shape::DateTime))
        .map(skwir_core::words::Moment::said)
        .unwrap_or_default()
}

/// How many files a place holds, said.
fn files(count: u64) -> String {
    words!(
        print_place_files = "{ $count ->
            [one] { $count } file
           *[other] { $count } files
        }",
        count = count,
        note = "In a printed listing's Size column: how many files a place listed (a tag) holds."
    )
    .say_plain()
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use skwir_core::app::{App, AppConfig};
    use skwir_core::command::Invocation;
    use skwir_core::location::Location;
    use skwir_core::print::Part;

    fn app(root: &std::path::Path) -> Arc<App> {
        App::new(AppConfig::ephemeral(root.join("state"))).unwrap()
    }

    /// A folder's listing is its entries as the pane shows them, each
    /// with its kind, size and time, nothing read again for it; filtered,
    /// what the filter keeps, its words in the title.
    #[test]
    fn a_folder_s_listing_is_what_its_pane_shows() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().join("letters 'n' notes");
        std::fs::create_dir_all(dir.join("drafts")).unwrap();
        std::fs::write(dir.join("b note.txt"), "hello").unwrap();
        std::fs::write(dir.join("a ünïcode.md"), "# Title\n").unwrap();
        std::fs::write(dir.join("c data.zqx9"), "?").unwrap();
        let app = app(tmp.path());
        let navigate = Invocation::with_args(
            "navigate",
            &serde_json::json!({ "location": dir.to_string_lossy() }),
        );
        app.dispatch(&navigate).unwrap();
        let pane = app.workspace.read().active_id();
        assert!(app.pump_until_complete(pane, std::time::Duration::from_secs(10)));
        let order: Vec<String> = {
            let ws = app.workspace.read();
            let shown = ws.pane(pane).unwrap();
            shown
                .order()
                .iter()
                .map(|&id| shown.entry(id).unwrap().name.to_string_lossy().into_owned())
                .collect()
        };
        let cancel = skwir_core::cancel::CancelToken::new();
        let listed = super::listing(&app, pane, &cancel).unwrap();
        assert_eq!(listed.title, dir.display().to_string());
        let Part::Table { columns, rows } = listed.table else {
            panic!("a table");
        };
        assert_eq!(columns, ["Name", "Type", "Size", "Modified"]);
        // In the pane's order, a folder marked as one.
        let names: Vec<&str> = rows
            .iter()
            .map(|row| row[0].trim_end_matches('/'))
            .collect();
        assert_eq!(names, order);
        let row = |name: &str| rows.iter().find(|row| row[0] == name).unwrap();
        assert_eq!(row("drafts/")[1], "Folder");
        assert_eq!(row("drafts/")[2], "", "a folder's size is not said");
        assert_eq!(row("b note.txt")[2], "5\u{a0}B");
        assert!(!row("b note.txt")[1].is_empty(), "its kind");
        assert_eq!(
            row("c data.zqx9")[1],
            "ZQX9 file",
            "a type named not, as the Type column says it"
        );
        assert!(
            !row("a ünïcode.md")[3].is_empty(),
            "when it was modified, as the window writes dates"
        );

        // Filtered: what the filter keeps, its words said.
        app.dispatch(&Invocation::with_args(
            "filter",
            &serde_json::json!({ "text": "note", "pane": pane }),
        ))
        .unwrap();
        let listed = super::listing(&app, pane, &cancel).unwrap();
        assert_eq!(listed.title, format!("{}, filtered by note", dir.display()));
        let Part::Table { rows, .. } = listed.table else {
            panic!("a table");
        };
        let names: Vec<&str> = rows.iter().map(|row| row[0].as_str()).collect();
        assert_eq!(names, ["b note.txt"]);
    }

    /// A place that is no folder (here one of files from two folders,
    /// whose provider restores them) is listed by its name, each file with
    /// the folder it is in and when it was deleted, read where the place
    /// listed none of it; a place it lists says its kind and count.
    #[test]
    fn a_place_s_listing_says_where_its_files_are() {
        let tmp = tempfile::tempdir().unwrap();
        let (one, two) = (tmp.path().join("one"), tmp.path().join("two"));
        std::fs::create_dir_all(&one).unwrap();
        std::fs::create_dir_all(&two).unwrap();
        std::fs::write(one.join("it's here.txt"), "hello").unwrap();
        std::fs::write(two.join("ünïcode.md"), "# Title\n").unwrap();
        // A folder and a file named to read as others where their U+202E
        // is obeyed.
        let spoof = tmp.path().join("a\u{202e}gpj.exe");
        std::fs::create_dir_all(&spoof).unwrap();
        std::fs::write(spoof.join("b\u{202e}txt.exe"), "x").unwrap();
        let app = app(tmp.path());
        let files = vec![
            one.join("it's here.txt"),
            two.join("ünïcode.md"),
            spoof.join("b\u{202e}txt.exe"),
        ];
        app.register_provider("test", Arc::new(fake::Bin { files }));
        let pane = app.workspace.read().active_id();
        app.navigate(pane, &Location::new(fake::SCHEME, ""))
            .unwrap();
        assert!(app.pump_until_complete(pane, std::time::Duration::from_secs(10)));
        let cancel = skwir_core::cancel::CancelToken::new();
        let listed = super::listing(&app, pane, &cancel).unwrap();
        assert_eq!(listed.title, "Bin of tests");
        let Part::Table { columns, rows } = listed.table else {
            panic!("a table");
        };
        assert_eq!(
            columns,
            ["Name", "Folder", "Deleted", "Type", "Size", "Modified"]
        );
        let row = |name: &str| rows.iter().find(|row| row[0] == name).unwrap();
        let here = row("it's here.txt");
        assert_eq!(
            here[1],
            skwir_core::os_path::from_home(&one).controls_shown()
        );
        assert_eq!(here[2], "", "no time said is none printed");
        assert_eq!(here[4], "5\u{a0}B", "read where the place listed none");
        assert_eq!(
            row("ünïcode.md")[1],
            skwir_core::os_path::from_home(&two).controls_shown()
        );
        // Its controls shown, never obeyed, in its name and its folder.
        let shown = row("b⟨RLO⟩txt.exe");
        assert!(shown[1].ends_with("a⟨RLO⟩gpj.exe"), "{shown:?}");
        assert!(
            !shown.iter().any(|cell| cell.contains('\u{202e}')),
            "{shown:?}"
        );
        let tag = row("a tag/");
        assert_eq!(tag[1], "", "a place is in no folder");
        assert_eq!(tag[3], "Tag");
        assert_eq!(tag[4], "3 files");
    }

    /// A listing names each type once, however many of its files it
    /// lists: a type's name is read of the type database (or asked of the
    /// registry) once a listing, never once a row.
    #[test]
    fn a_listing_reads_each_type_s_name_once() {
        use skwir_core::FileId;
        use skwir_core::entry::{Entry, EntryId, EntryKind};
        let names = ["a.txt", "b.txt", "c.txt", "d.zqx9", "e.zqx9", "f"];
        let entries: Vec<Entry> = (0u64..)
            .zip(names)
            .map(|(n, name)| {
                let id = EntryId::at_path(FileId::new(7, n), std::path::Path::new(name));
                Entry::new(id, name, EntryKind::File)
            })
            .collect();
        thread_local! {
            /// Each type whose name was read, as many times as it was.
            static READ: std::cell::RefCell<Vec<skwir_platform::mime::Mime>> =
                const { std::cell::RefCell::new(Vec::new()) };
        }
        fn counted(mime: &skwir_platform::mime::Mime) -> Option<String> {
            READ.with_borrow_mut(|read| read.push(mime.clone()));
            mime.description()
        }
        let mut said = super::Said::describing(counted);
        let kinds: Vec<String> = entries.iter().map(|entry| said.type_of(entry)).collect();
        assert_eq!(kinds[0], kinds[1]);
        assert_eq!(kinds[1], kinds[2]);
        assert_eq!(kinds[3], "ZQX9 file", "a type named not, by its extension");
        assert_eq!(kinds[4], "ZQX9 file");
        let types: std::collections::HashSet<_> = entries
            .iter()
            .filter_map(skwir_platform::mime::of_entry)
            .collect();
        // Every type read, each once, however many rows are of it.
        let read = READ.take();
        let once: std::collections::HashSet<_> = read.iter().cloned().collect();
        assert_eq!(read.len(), once.len(), "a type read twice: {read:?}");
        assert_eq!(once, types, "{read:?}");
        // One an extension at most: ZQX9, none, and TXT where the system
        // names not text's type (Wine's registry).
        assert!(said.unnamed.len() <= 3, "{:?}", said.unnamed);
    }

    mod fake {
        use skwir_core::words::Words;
        use std::collections::HashMap;
        use std::path::PathBuf;
        use std::sync::Arc;

        use crossbeam_channel::Receiver;
        use skwir_core::FileId;
        use skwir_core::command::Invocation;
        use skwir_core::entry::{Entry, EntryId, EntryKind, Provenance, Value, keys};
        use skwir_core::location::{
            EventSink, ListingEvent, Location, LocationProvider, OpenContext, OpenLocation, Verbs,
        };
        use skwir_core::scheduler::Priority;

        pub const SCHEME: &str = "test-bin";

        /// A place of files from several folders, given by their paths
        /// without their metadata, beside a place of its own; it restores
        /// what it lists, as the trash does.
        pub struct Bin {
            pub files: Vec<PathBuf>,
        }

        impl LocationProvider for Bin {
            fn scheme(&self) -> &str {
                SCHEME
            }

            fn title(&self, _: &Location) -> Option<skwir_core::words::Words> {
                Some(Words::as_written("Bin of tests"))
            }

            fn verbs(&self, _: &Location) -> Verbs {
                Verbs {
                    restore: Some(Invocation::new("test.restore")),
                    ..Verbs::default()
                }
            }

            fn open(
                &self,
                location: &Location,
                ctx: OpenContext,
            ) -> anyhow::Result<Arc<dyn OpenLocation>> {
                let (sink, rx) = ctx.channel();
                let mut paths = HashMap::new();
                let mut entries: Vec<Entry> = Vec::new();
                for (n, path) in (1u64..).zip(&self.files) {
                    let id = EntryId::at_path(FileId::new(7, n), path);
                    paths.insert(id, path.clone());
                    entries.push(Entry::new(id, path.file_name().unwrap(), EntryKind::File));
                }
                let mut tag = Entry::new(EntryId::place(1), "a tag", EntryKind::Directory);
                tag.metadata.set(
                    keys::PLACE_KIND,
                    Value::Text("Tag".into()),
                    Provenance::Plugin {
                        name: "test".into(),
                    },
                );
                tag.metadata
                    .set(keys::FILES, Value::UInt(3), Provenance::Filesystem);
                entries.push(tag);
                sink.send(ListingEvent::Added(entries));
                sink.send(ListingEvent::Complete);
                Ok(Arc::new(Open {
                    location: location.clone(),
                    rx,
                    _sink: sink,
                    paths,
                }))
            }
        }

        struct Open {
            location: Location,
            rx: Receiver<ListingEvent>,
            _sink: EventSink,
            paths: HashMap<EntryId, PathBuf>,
        }

        impl OpenLocation for Open {
            fn location(&self) -> &Location {
                &self.location
            }

            fn events(&self) -> &Receiver<ListingEvent> {
                &self.rx
            }

            fn hydrate(&self, _: &[EntryId], _: Priority) {}

            fn path_of(&self, id: EntryId) -> Option<PathBuf> {
                self.paths.get(&id).cloned()
            }

            fn refresh(&self) {}

            fn close(&self) {}
        }
    }
}
