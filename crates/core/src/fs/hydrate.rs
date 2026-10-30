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
/// location listed them by) and one `Removed` for what is gone, and
/// returns the gone ids so the caller can forget their paths. What cannot
/// be read is not answered: its row stays as it was. Worker-only: this is
/// I/O. Nothing more is sent once `cancel` fires.
pub fn hydrate_paths(
    items: &[(EntryId, PathBuf)],
    metadata: &MetadataProviders,
    sink: &EventSink,
    cancel: &CancelToken,
) -> Vec<EntryId> {
    let mut updated = Vec::with_capacity(items.len());
    let mut gone = Vec::new();
    let read = read_paths(items, metadata, cancel, |mut found, mut went| {
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
    let mut by_dir: HashMap<&Path, Vec<(EntryId, OsString)>> = HashMap::new();
    let mut gone = Vec::new();
    for (id, path) in items {
        match (path.parent(), path.file_name()) {
            (Some(dir), Some(name)) => by_dir
                .entry(dir)
                .or_default()
                .push((*id, name.to_os_string())),
            _ => gone.push(*id),
        }
    }
    for (dir, names) in by_dir {
        if cancel.is_cancelled() {
            return false;
        }
        let folder = match Dir::open(dir) {
            Ok(folder) => folder,
            Err(error)
                if matches!(
                    error.kind(),
                    std::io::ErrorKind::NotFound | std::io::ErrorKind::NotADirectory
                ) =>
            {
                gone.extend(names.into_iter().map(|(id, _)| id));
                continue;
            }
            // A folder that cannot be read: what was known of its entries
            // stands.
            Err(_) => continue,
        };
        let mut found = Vec::with_capacity(names.len());
        for (id, name) in names {
            let mut e = Entry::new(id, name, EntryKind::Unknown);
            match stat_entry_as(&folder, &mut e) {
                Stated::Found => {
                    e.id = id;
                    found.push(e);
                }
                Stated::Gone => gone.push(id),
                // There, unread: what was known of it stands.
                Stated::Unread => {}
            }
        }
        metadata.fill(dir, &mut found);
        read(found, std::mem::take(&mut gone));
    }
    if !gone.is_empty() {
        read(Vec::new(), gone);
    }
    !cancel.is_cancelled()
}
