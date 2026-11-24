//! Hydration for locations whose entries live in many folders (search
//! results, the shelf, recent files): the same stat and metadata pass the
//! path provider runs, keyed by path instead of by name in one folder.

use std::collections::HashMap;
use std::ffi::OsString;
use std::path::{Path, PathBuf};

use crate::cancel::CancelToken;
use crate::entry::{Entry, EntryId, EntryKind};
use crate::location::{EventSink, ListingEvent};
use crate::metadata::MetadataProviders;

use super::dir::Dir;
use super::newest::Tickets;
use super::stat::{Stated, stat_entry_as, stat_path};

/// One path read as a listing reads its entries: stat-ed, then filled by
/// the metadata providers (its tags, its rating), so what describes a
/// path alone (the folder a pane shows) knows what its row would.
/// Worker-only: this is I/O.
pub fn hydrate_path(path: &Path, metadata: &MetadataProviders) -> Option<Entry> {
    let mut entry = stat_path(path)?;
    // The root is named by its whole path, which joins onto anything as
    // itself.
    let dir = path.parent().unwrap_or(path);
    metadata.fill(dir, std::slice::from_mut(&mut entry));
    Some(entry)
}

/// What one hydration job of a location of many folders takes at most
/// (`OpenLocation::hydrate`): a request for more is several jobs, run side
/// by side on the workers, where one job stat-ed them all in turn.
pub const HYDRATE_JOB: usize = 512;

/// Entries found are sent at least this many at a time as folders are
/// read, so the window applies them in slices as they come rather than in
/// one event it cannot split.
const SENT_EVERY: usize = 256;

/// Stat `items` and run the metadata providers over them, one folder at a
/// time so each folder is opened once and providers get a batch. Its
/// caller runs it in the lane of the items' paths, which share one
/// (`reach::by_lane`, ADR-077): one remote that stopped answering would
/// hold the files of every other folder behind it. Sends
/// `Updated` for what was found as it goes (under the ids given, which the
/// location listed them by), one `Removed` for what is gone and one
/// `Unreadable` for what is there and cannot be read: every id given is
/// answered, so nothing waits for one. Returns the gone ids so the caller
/// can forget their paths. Worker-only: this is I/O. Nothing more is sent
/// once `cancel` fires.
pub fn hydrate_paths(
    items: &[(EntryId, PathBuf)],
    metadata: &MetadataProviders,
    sink: &EventSink,
    cancel: &CancelToken,
) -> Vec<EntryId> {
    hydrate_paths_with(items, metadata, sink, cancel, |_, _| {})
}

/// [`hydrate_paths`], `mark` given each folder's entries found, and the
/// folder, before they are sent: what the location says of an entry that
/// no stat does (`Entry::in_hidden_folder`).
pub fn hydrate_paths_with(
    items: &[(EntryId, PathBuf)],
    metadata: &MetadataProviders,
    sink: &EventSink,
    cancel: &CancelToken,
    mut mark: impl FnMut(&Path, &mut [Entry]),
) -> Vec<EntryId> {
    let folders: crate::hashing::Map<EntryId, &Path> = items
        .iter()
        .filter_map(|(id, path)| Some((*id, path.parent()?)))
        .collect();
    let mut updated = Vec::with_capacity(items.len());
    let mut gone = Vec::new();
    let mut answered = crate::hashing::Set::default();
    let read = read_paths(items, metadata, cancel, |mut found, mut went| {
        // One folder's at a time.
        if let Some(dir) = found.first().and_then(|e| folders.get(&e.id)) {
            mark(dir, &mut found);
        }
        answered.extend(found.iter().map(|e| e.id).chain(went.iter().copied()));
        updated.append(&mut found);
        gone.append(&mut went);
        if updated.len() >= SENT_EVERY && !cancel.is_cancelled() {
            sink.send(ListingEvent::Updated(std::mem::take(&mut updated)));
        }
    });
    if !read {
        return Vec::new();
    }
    if !updated.is_empty() {
        sink.send(ListingEvent::Updated(updated));
    }
    if !gone.is_empty() {
        sink.send(ListingEvent::Removed(gone.clone()));
    }
    let unreadable: Vec<EntryId> = items
        .iter()
        .map(|(id, _)| *id)
        .filter(|id| !answered.contains(id))
        .collect();
    if !unreadable.is_empty() {
        sink.send(ListingEvent::Unreadable(unreadable));
    }
    gone
}

/// Read `items` as `hydrate_paths` does, one folder at a time, handing
/// `read` each folder's: the entries found, under the ids given, and the
/// ids of those gone. What is neither is there and cannot be read (a folder
/// that cannot be opened, a stat refused): what was known of it stands.
/// For a caller that tests what it reads instead of sending it (a search
/// within a place). False once `cancel` fires: nothing more is read.
/// Worker-only: this is I/O, in the lane of the items' paths.
pub fn read_paths(
    items: &[(EntryId, PathBuf)],
    metadata: &MetadataProviders,
    cancel: &CancelToken,
    mut read: impl FnMut(Vec<Entry>, Vec<EntryId>),
) -> bool {
    read_paths_newest(items, metadata, cancel, &Tickets::default(), |r| {
        if !r.found.is_empty() || !r.gone.is_empty() {
            read(
                r.found.into_iter().map(|(e, _)| e).collect(),
                r.gone.into_iter().map(|(id, _)| id).collect(),
            );
        }
    })
}

/// What [`read_paths_newest`] read of one folder's items, each with the
/// ticket its read took.
#[derive(Debug, Default)]
pub struct Read {
    /// Found, under the ids given, filled by the metadata providers.
    pub found: Vec<(Entry, u64)>,
    /// Gone, or never there (a path in no folder).
    pub gone: Vec<(EntryId, u64)>,
    /// There and unread (its folder cannot be opened, its stat was
    /// refused): what was known of it stands.
    pub unread: Vec<(EntryId, u64)>,
}

/// [`read_paths`], each item's read taking a ticket of `tickets` just
/// before it reads (a folder that cannot be opened, one for its items):
/// for a location that keeps the newest read of each entry
/// ([`Newest`](super::Newest)), which applies what `read` is handed under
/// its own lock, the one it sends under, only where no read of the entry
/// begun later was applied, its read counted under way meanwhile
/// ([`Reading`](super::newest::Reading)). Every item is handed once,
/// found, gone or unread. False once `cancel` fires: nothing more is read.
/// Worker-only: this is I/O, in the lane of the items' paths.
pub fn read_paths_newest(
    items: &[(EntryId, PathBuf)],
    metadata: &MetadataProviders,
    cancel: &CancelToken,
    tickets: &Tickets,
    mut read: impl FnMut(Read),
) -> bool {
    let mut by_dir: HashMap<&Path, Vec<(EntryId, OsString)>> = HashMap::new();
    let mut nowhere = Vec::new();
    for (id, path) in items {
        match (path.parent(), path.file_name()) {
            (Some(dir), Some(name)) => by_dir
                .entry(dir)
                .or_default()
                .push((*id, name.to_os_string())),
            _ => nowhere.push(*id),
        }
    }
    for (dir, names) in by_dir {
        if cancel.is_cancelled() {
            return false;
        }
        let opening = tickets.take();
        let folder = match Dir::open(dir) {
            Ok(folder) => folder,
            Err(error)
                if matches!(
                    error.kind(),
                    std::io::ErrorKind::NotFound | std::io::ErrorKind::NotADirectory
                ) =>
            {
                read(Read {
                    gone: names.into_iter().map(|(id, _)| (id, opening)).collect(),
                    ..Read::default()
                });
                continue;
            }
            // A folder that cannot be read: what was known of its entries
            // stands.
            Err(_) => {
                read(Read {
                    unread: names.into_iter().map(|(id, _)| (id, opening)).collect(),
                    ..Read::default()
                });
                continue;
            }
        };
        let mut r = Read::default();
        let mut found = Vec::with_capacity(names.len());
        for (id, name) in names {
            let mut e = Entry::new(id, name, EntryKind::Unknown);
            let ticket = tickets.take();
            match stat_entry_as(&folder, &mut e) {
                Stated::Found => {
                    e.id = id;
                    found.push((e, ticket));
                }
                Stated::Gone => r.gone.push((id, ticket)),
                // There, unread: what was known of it stands.
                Stated::Unread => r.unread.push((id, ticket)),
            }
        }
        let (mut entries, taken): (Vec<Entry>, Vec<u64>) = found.into_iter().unzip();
        metadata.fill(dir, &mut entries);
        r.found = entries.into_iter().zip(taken).collect();
        read(r);
    }
    if !nowhere.is_empty() {
        let ticket = tickets.take();
        read(Read {
            gone: nowhere.into_iter().map(|id| (id, ticket)).collect(),
            ..Read::default()
        });
    }
    !cancel.is_cancelled()
}
