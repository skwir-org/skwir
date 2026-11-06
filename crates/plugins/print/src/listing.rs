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
        let pane = ws.pane(pane).context("No such pane")?;
        let location = pane.location().cloned().context("The pane shows nothing")?;
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
                let app = weak.upgrade().context("Skwir is closing")?;
                let mut found = Vec::new();
                let finished =
                    skwir_core::fs::read_paths(&items, &app.metadata, &token, |entries, _| {
                        found.extend(entries);
                    });
                anyhow::ensure!(finished, "Printing was cancelled");
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
    let zone = jiff::tz::TimeZone::system();
    let rows = shown
        .iter()
        .map(|(entry, depth, path)| row(entry, *depth, path.as_deref(), columns, &zone))
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
/// place's name, then what filters it.
fn title(app: &App, location: &Location, filter: Option<&str>) -> String {
    let named = location.path().map_or_else(
        || app.location_title(location),
        |dir| dir.display().to_string(),
    );
    filtered(named, filter)
}

/// `named` with what filters it, as typed over it (`Pane::filter`).
pub(crate) fn filtered(named: String, filter: Option<&str>) -> String {
    match filter.map(str::trim).filter(|words| !words.is_empty()) {
        Some(words) => format!("{named}, filtered by {words}"),
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
    fn names(self) -> Vec<String> {
        let mut names = vec!["Name"];
        if self.folder {
            names.push("Folder");
        }
        if self.deleted {
            names.push("Deleted");
        }
        names.extend(["Kind", "Size", "Modified"]);
        names.into_iter().map(str::to_owned).collect()
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
    zone: &jiff::tz::TimeZone,
) -> Vec<String> {
    let mut name = "    ".repeat(depth);
    name.push_str(&entry.name.to_string_lossy());
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
        row.push(folder.map_or_else(String::new, |dir| skwir_core::os_path::from_home(&dir)));
    }
    if columns.deleted {
        row.push(when(entry.metadata.value(keys::TRASHED), zone));
    }
    let kind = if entry.is_place() {
        entry
            .metadata
            .value(keys::PLACE_KIND)
            .and_then(Value::as_str)
            .unwrap_or("Place")
            .to_owned()
    } else if entry.is_dir() {
        "Folder".to_owned()
    } else {
        skwir_platform::mime::of_entry(entry)
            .and_then(|mime| mime.description())
            .unwrap_or_default()
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
    row.extend([kind, size, when(entry.metadata.value(keys::MODIFIED), zone)]);
    row
}

/// A time a field holds, to the minute in the system's zone; blank when
/// none is known.
fn when(value: Option<&Value>, zone: &jiff::tz::TimeZone) -> String {
    value
        .and_then(Value::as_time)
        .and_then(|time| jiff::Timestamp::try_from(time).ok())
        .map(|at| {
            at.to_zoned(zone.clone())
                .strftime("%Y-%m-%d %H:%M")
                .to_string()
        })
        .unwrap_or_default()
}

/// How many files a place holds, said.
fn files(count: u64) -> String {
    if count == 1 {
        "1 file".to_owned()
    } else {
        format!("{count} files")
    }
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
        assert_eq!(columns, ["Name", "Kind", "Size", "Modified"]);
        // In the pane's order, a folder marked as one.
        let names: Vec<&str> = rows
            .iter()
            .map(|row| row[0].trim_end_matches('/'))
            .collect();
        assert_eq!(names, order);
        let row = |name: &str| rows.iter().find(|row| row[0] == name).unwrap();
        assert_eq!(row("drafts/")[1], "Folder");
        assert_eq!(row("drafts/")[2], "", "a folder's size is not said");
        assert_eq!(row("b note.txt")[2], "5 B");
        assert!(!row("b note.txt")[1].is_empty(), "its kind");
        assert_eq!(row("a ünïcode.md")[3].len(), "2026-10-31 12:00".len());

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
        let app = app(tmp.path());
        let files = vec![one.join("it's here.txt"), two.join("ünïcode.md")];
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
            ["Name", "Folder", "Deleted", "Kind", "Size", "Modified"]
        );
        let row = |name: &str| rows.iter().find(|row| row[0] == name).unwrap();
        let here = row("it's here.txt");
        assert_eq!(here[1], skwir_core::os_path::from_home(&one));
        assert_eq!(here[2], "", "no time said is none printed");
        assert_eq!(here[4], "5 B", "read where the place listed none");
        assert_eq!(row("ünïcode.md")[1], skwir_core::os_path::from_home(&two));
        let tag = row("a tag/");
        assert_eq!(tag[1], "", "a place is in no folder");
        assert_eq!(tag[3], "Tag");
        assert_eq!(tag[4], "3 files");
    }

    mod fake {
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

            fn title(&self, _: &Location) -> Option<String> {
                Some("Bin of tests".to_owned())
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
