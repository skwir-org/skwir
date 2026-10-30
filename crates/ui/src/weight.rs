//! What the files a pane shows weigh together, for its footer while
//! nothing is selected: counted as a selection's size is (the files' own
//! sizes; folders are not walked, that is the Usage view).
//!
//! The listing cannot say: a big folder learns the sizes of the rows it
//! draws, and a sum of those is a wrong number that grows as it scrolls.
//! So once its listing is complete a worker adds up the files the pane
//! shows (`Pane::order`: hidden files only when it shows them, the filter
//! applied): each as the listing has stat-ed it (a row drawn, a file the
//! watcher saw change), else as a stat of its own found it, kept by its
//! entry for as long as the pane holds that listing (`Known`), else
//! stat-ed now and kept there. What the pane shows changing (a name typed
//! in the filter, hidden files shown, a file come or gone) adds up again
//! and stats only what neither knows: a filter typed over a hundred
//! thousand files reads nothing, a file arrived is stat-ed alone. A size
//! the listing learns (a file that grows, rows hydrated as they scroll)
//! adds up again once the request running has answered, never starting it
//! over: starting over for each batch of metadata, a big folder's weight
//! was never said. A request otherwise supersedes the one before; both end
//! with the location (`Pane::location_lifetime`). Where the pane asks for
//! every file's metadata anyway (a narrowing, a sort by size or date),
//! what it shows is added up from the listing once each file has it, and
//! again when a size changes: nothing is read twice. Until then each batch
//! of metadata asks whether a file is still unknown, from where the last
//! one was found (`Pane::file_unknown_from`), so a big folder is not walked
//! per batch. The footer keeps the last answer for the same folder until
//! the next, so it does not blink.

use std::collections::{HashMap, HashSet};
use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use gpui::{Context, Window};
use skwir_core::workspace::PaneId;
use skwir_core::{App, CancelToken, EntryId, EntryKind, FileId};

use crate::shell::Shell;

/// What a sum was asked for: the listing, its names, its sizes, and what
/// the pane shows of it (hidden files, the filter's version). A listing is
/// one location's: navigating makes another. Read from the folder, the
/// order the pane shows is what is added up (`Weight::order`).
#[derive(Clone, Copy, PartialEq, Eq)]
struct Key {
    listing: u64,
    names: u64,
    sizes: u64,
    hidden: bool,
    filter: u64,
    from_listing: bool,
}

/// A pane's weight: the last sum, the folder it is of, and what it was, or
/// is being, asked for.
#[derive(Default)]
pub struct Weight {
    key: Option<Key>,
    /// The order last added up from the folder: what the pane showed.
    order: Option<Arc<[EntryId]>>,
    dir: Option<PathBuf>,
    bytes: Option<u64>,
    cancel: Option<CancelToken>,
    /// Which request an answer is for: only the last one's is taken.
    asked: u64,
    /// A size changed while a request ran, which adds up again once it
    /// answers.
    again: bool,
    /// Where a file without its metadata was last found in the pane's
    /// order: the next look starts there.
    unknown_at: usize,
    /// What the folder's files were found to weigh, for the listing.
    known: Option<Arc<Known>>,
}

/// What each file of a listing weighs, by its entry, as stat-ed: kept for
/// as long as the pane holds the listing, read and filled by its workers
/// only, one request at a time.
struct Known {
    listing: u64,
    by_id: parking_lot::Mutex<HashMap<EntryId, Weighed>>,
}

/// What an entry of the folder weighs here (`Entry::weighed`): a file its
/// size, as the file it is (two names of one file are one file); a
/// folder, or a link to one, nothing.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Weighed {
    Nothing,
    File(FileId, u64),
}

impl Weighed {
    /// What `e`, stat-ed, weighs: a link its target's size, a link to a
    /// folder nothing.
    fn of(e: &skwir_core::Entry) -> Self {
        e.weighed()
            .map_or(Weighed::Nothing, |(file, size)| Weighed::File(file, size))
    }
}

/// What an entry the pane shows weighs now, as `now` tells it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Now {
    Weighs(Weighed),
    /// Listed, stat-ed by no one yet.
    Unknown,
    /// Gone from the listing since the order was read.
    Gone,
}

/// What entry `listed` of the folder's listing weighs now: as the listing
/// stat-ed it, which stays current (the watcher's changes come stat-ed,
/// rows drawn are hydrated), else as it was `known` here. One the listing
/// does not hold weighs nothing when it is `elsewhere` among the pane's (a
/// folder opened in place has its own), and is gone otherwise.
fn now(
    listed: Option<&skwir_core::Entry>,
    elsewhere: impl FnOnce() -> bool,
    known: Option<&Weighed>,
) -> Now {
    match listed {
        Some(e) if e.kind == EntryKind::Directory => Now::Weighs(Weighed::Nothing),
        Some(e) if e.metadata.is_hydrated() => Now::Weighs(Weighed::of(e)),
        Some(_) => known.map_or(Now::Unknown, |known| Now::Weighs(*known)),
        None if elsewhere() => Now::Weighs(Weighed::Nothing),
        None => Now::Gone,
    }
}

/// Bytes added up, each file once however many of its names are shown
/// (ADR-049).
#[derive(Default)]
struct Total {
    bytes: u64,
    seen: HashSet<FileId>,
}

impl Total {
    fn add(&mut self, weighed: Weighed) {
        if let Weighed::File(file, size) = weighed
            && self.seen.insert(file)
        {
            self.bytes = self.bytes.saturating_add(size);
        }
    }
}

/// How many entries a worker reads of the listing under its lock at once.
const SLICE: usize = 4096;
/// How many files a worker stats before it keeps what they weigh: a
/// request superseded meanwhile loses no more.
const STATS: usize = 256;

impl Shell {
    /// Ask for the weight of what the pane being rendered shows, when that
    /// changed. Every frame costs one key of numbers and an order's
    /// identity compared; the location is read only when it moved.
    pub(crate) fn sync_weight(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let pane = self.pane;
        let key = {
            let ws = self.app.workspace.read();
            let Some(p) = ws.pane(pane) else { return };
            // A folder still listing keeps what it had until it is complete.
            if !p.listing.is_complete() {
                return;
            }
            // A folder sorted by size or date (another location is not
            // weighed), or any filtered by what the files' metadata says.
            let folder = p.location().is_some_and(|l| l.path().is_some());
            let reads = p.read_filter().is_some_and(|r| r.metadata);
            let from_listing = reads || folder && p.sort.key.needs_metadata();
            Key {
                listing: p.listing.id(),
                names: p.listing.names_version(),
                sizes: p.listing.size_version(),
                hidden: p.sort.show_hidden,
                filter: p.filter_version(),
                from_listing,
            }
        };
        // What the pane shows, as its views draw it (`ListState::sync`).
        let order = Arc::clone(&self.ui().list.sorted);
        let weight = &self.ui().weight;
        let same_order = weight
            .order
            .as_ref()
            .is_some_and(|o| Arc::ptr_eq(o, &order));
        if weight.key == Some(key) && (key.from_listing || same_order) {
            return;
        }
        // Only sizes learnt while a request reads the folder: it goes on,
        // and adds up again once it answers.
        if !key.from_listing
            && same_order
            && weight.cancel.is_some()
            && weight.key.is_some_and(|was| {
                Key {
                    sizes: key.sizes,
                    ..was
                } == key
            })
        {
            self.ui_mut().weight.again = true;
            return;
        }
        let unknown_at = weight.unknown_at;
        let (asked, listed) = {
            let ws = self.app.workspace.read();
            let Some(p) = ws.pane(pane) else { return };
            (
                p.location()
                    .and_then(skwir_core::Location::path)
                    .map(|dir| (dir, p.location_lifetime())),
                key.from_listing.then(|| listed_bytes(p, unknown_at)),
            )
        };
        let weight = &mut self.ui_mut().weight;
        if let Some(cancel) = weight.cancel.take() {
            cancel.cancel();
        }
        weight.key = Some(key);
        weight.asked += 1;
        weight.again = false;
        if let Some(listed) = listed {
            weight.order = None;
            let dir = asked.map(|(dir, _)| dir);
            if weight.dir != dir {
                weight.dir = dir;
                weight.bytes = None;
            }
            // Until every file has its size, the last answer stays.
            match listed {
                Listed::Unknown(at) => weight.unknown_at = at,
                Listed::Bytes(bytes) => weight.bytes = Some(bytes),
            }
            return;
        }
        let Some((dir, lifetime)) = asked else {
            // Not a folder: nothing to weigh.
            weight.dir = None;
            weight.bytes = None;
            weight.order = None;
            weight.known = None;
            return;
        };
        // Another folder: what was said of the one before is not its own.
        if weight.dir.as_ref() != Some(&dir) {
            weight.bytes = None;
            weight.dir = Some(dir.clone());
            weight.known = None;
        }
        // What its files weigh is known for the listing it was read for:
        // another, even of the same folder (F5), reads them again.
        let known = match &weight.known {
            Some(known) if known.listing == key.listing => Arc::clone(known),
            _ => {
                let known = Arc::new(Known {
                    listing: key.listing,
                    by_id: parking_lot::Mutex::default(),
                });
                weight.known = Some(Arc::clone(&known));
                known
            }
        };
        weight.order = Some(Arc::clone(&order));
        let cancel = lifetime.child();
        weight.cancel = Some(cancel.clone());
        let asked = weight.asked;
        let (tx, rx) = flume::bounded(1);
        let token = cancel.clone();
        let app = Arc::clone(&self.app);
        // A stat for each file not known: in the folder's lane, so a
        // server that stopped answering holds its own.
        self.app.scheduler.spawn_in(
            skwir_core::fs::reach::lane(&dir),
            skwir_core::scheduler::Priority::Background,
            "folder-weight",
            cancel,
            move || {
                if let Some(bytes) = weigh_shown(&app, pane, &dir, &order, &known.by_id, &token) {
                    let _ = tx.send(bytes);
                }
            },
        );
        cx.spawn_in(window, async move |this, cx| {
            // No answer: cancelled, or the folder could not be read. Ended
            // all the same, so a size learnt meanwhile is not left waiting.
            let answer = rx.recv_async().await.ok();
            this.update_in(cx, |shell, _, cx| {
                if let Some(ui) = shell.pane_ui_mut(pane)
                    && ui.weight.asked == asked
                {
                    let changed = answer.is_some_and(|bytes| ui.weight.bytes != Some(bytes));
                    if answer.is_some() {
                        ui.weight.bytes = answer;
                    }
                    ui.weight.cancel = None;
                    // A size learnt meanwhile is added up at the next frame.
                    if changed || ui.weight.again {
                        cx.notify();
                    }
                }
            })
            .ok();
        })
        .detach();
    }

    /// What the files the pane being rendered shows weigh, once known.
    pub(crate) fn listed_bytes(&self) -> Option<u64> {
        self.ui().weight.bytes
    }
}

/// What the listing says of what the files a pane shows weigh.
enum Listed {
    /// A file has no metadata yet; the first found is there in the order.
    Unknown(usize),
    Bytes(u64),
}

/// What the files the pane shows weigh, once each has its metadata,
/// looking for one without from `unknown_at` on: each file once, its
/// names (hard links) one file (`entry::weight`).
fn listed_bytes(p: &skwir_core::workspace::Pane, unknown_at: usize) -> Listed {
    if let Some(at) = p.file_unknown_from(unknown_at) {
        return Listed::Unknown(at);
    }
    Listed::Bytes(skwir_core::entry::weight(
        p.order().iter().filter_map(|id| p.entry(*id)),
    ))
}

/// What the files `order` shows of the folder `dir` (pane `pane`'s listing)
/// weigh (`now`): what neither the listing nor `known` says is stat-ed and
/// kept there. What left the listing meanwhile is left out. `None` once
/// cancelled, or when the folder cannot be read.
fn weigh_shown(
    app: &App,
    pane: PaneId,
    dir: &Path,
    order: &[EntryId],
    known: &parking_lot::Mutex<HashMap<EntryId, Weighed>>,
    cancel: &CancelToken,
) -> Option<u64> {
    let mut total = Total::default();
    let mut missing: Vec<(EntryId, OsString, EntryKind)> = Vec::new();
    // Read under the workspace's lock a slice at a time (`known`'s taken
    // after it, always in that order), let go before any is stat-ed.
    for slice in order.chunks(SLICE) {
        if cancel.is_cancelled() {
            return None;
        }
        let ws = app.workspace.read();
        let p = ws.pane(pane)?;
        let known = known.lock();
        for &id in slice {
            let listed = p.listing.get(id);
            match now(listed, || p.entry(id).is_some(), known.get(&id)) {
                Now::Weighs(weighed) => total.add(weighed),
                Now::Unknown => {
                    if let Some(e) = listed {
                        missing.push((id, e.name.clone(), e.kind));
                    }
                }
                Now::Gone => {}
            }
        }
    }
    if !missing.is_empty() {
        let folder = skwir_core::fs::Dir::open(dir).ok()?;
        for names in missing.chunks(STATS) {
            let weighed = stat_names(&folder, names, cancel)?;
            for (_, w) in &weighed {
                total.add(*w);
            }
            known.lock().extend(weighed);
        }
    }
    // Entries gone from the listing are let go once they outnumber it:
    // the workspace's lock first, as above.
    let len = known.lock().len();
    if len > 1024 {
        let ws = app.workspace.read();
        if let Some(p) = ws.pane(pane)
            && len > 2 * p.listing.len() + 1024
        {
            known.lock().retain(|id, _| p.listing.get(*id).is_some());
        }
    }
    Some(total.bytes)
}

/// What each of `names` (an entry of the listing of `folder`: its id, name
/// and kind) weighs, stat-ed as the listing stats it (`fs::stat_entry`):
/// a link its target's size, a link to a folder nothing. Kept by the id
/// the listing knows it by, which a stat may correct (a provisional one).
/// One that could not be stat-ed is left out, to be tried again with the
/// next request. `None` once cancelled.
fn stat_names(
    folder: &skwir_core::fs::Dir,
    names: &[(EntryId, OsString, EntryKind)],
    cancel: &CancelToken,
) -> Option<Vec<(EntryId, Weighed)>> {
    let mut out = Vec::with_capacity(names.len());
    for (id, name, kind) in names {
        if cancel.is_cancelled() {
            return None;
        }
        let mut entry = skwir_core::Entry::new(EntryId::UNKNOWN, name.clone(), *kind);
        if skwir_core::fs::stat_entry(folder, &mut entry) {
            out.push((*id, Weighed::of(&entry)));
        }
    }
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn id(ino: u64) -> EntryId {
        EntryId {
            file: FileId::new(1, ino),
            at: ino,
        }
    }

    /// A folder of the tests' own, never /tmp (tmpfs).
    fn scratch(name: &str) -> PathBuf {
        // Windows' (and Wine's) home is its user's profile.
        let home = std::env::var_os("HOME").or_else(|| std::env::var_os("USERPROFILE"));
        let base = PathBuf::from(home.expect("home"))
            .join(".cache")
            .join("skwir-tests")
            .join(format!("weight-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        std::fs::create_dir_all(&base).expect("scratch");
        base
    }

    /// What `ids` weigh as the listing (`listing`, by id) and `known` say,
    /// and those neither does, as `weigh_shown` adds them up.
    fn add_up(
        ids: &[EntryId],
        listing: &HashMap<EntryId, skwir_core::Entry>,
        elsewhere: &[EntryId],
        known: &HashMap<EntryId, Weighed>,
    ) -> (u64, Vec<EntryId>) {
        let mut total = Total::default();
        let mut missing = Vec::new();
        for id in ids {
            match now(listing.get(id), || elsewhere.contains(id), known.get(id)) {
                Now::Weighs(weighed) => total.add(weighed),
                Now::Unknown => missing.push(*id),
                Now::Gone => {}
            }
        }
        (total.bytes, missing)
    }

    /// What is shown is added up as the listing stat-ed it, else as it was
    /// known here, a file once however many of its names are shown; what
    /// neither says is asked for, and only that: a filter that shows fewer
    /// reads nothing, and a file that grew weighs what the listing says.
    #[test]
    fn what_is_shown_is_added_up_from_what_is_known() {
        let file = |ino, size| Weighed::File(FileId::new(1, ino), size);
        let entry = |ino, name: &str, kind, size: Option<u64>| {
            let mut e = skwir_core::Entry::new(id(ino), name, kind);
            if let Some(size) = size {
                e.metadata.set(
                    skwir_core::entry::keys::SIZE,
                    skwir_core::Value::UInt(size),
                    skwir_core::Provenance::Filesystem,
                );
            }
            (id(ino), e)
        };
        let listing: HashMap<EntryId, skwir_core::Entry> = [
            entry(1, "it's a report.txt", EntryKind::File, None),
            entry(2, "notes.md", EntryKind::File, None),
            // A second name of file 2 (a hard link).
            (id(3), {
                let mut e = skwir_core::Entry::new(id(3), "notes again.md", EntryKind::File);
                e.id.file = FileId::new(1, 2);
                e
            }),
            entry(4, "folder", EntryKind::Directory, None),
            entry(5, "arrived", EntryKind::File, None),
            // Grown since it was stat-ed here: the watcher's stat says.
            entry(7, "growing.log", EntryKind::File, Some(500)),
        ]
        .into();
        let known: HashMap<EntryId, Weighed> = [
            (id(1), file(1, 300)),
            (id(2), file(2, 20)),
            (id(3), file(2, 20)),
            (id(7), file(7, 100)),
        ]
        .into();
        let shown = [id(1), id(2), id(3), id(4)];
        assert_eq!(add_up(&shown, &listing, &[], &known), (320, vec![]));
        assert_eq!(
            add_up(&[id(1)], &listing, &[], &known),
            (300, vec![]),
            "filtered: nothing read"
        );
        assert_eq!(
            add_up(&[id(5), id(1)], &listing, &[], &known),
            (300, vec![id(5)]),
            "arrived: that alone asked for"
        );
        assert_eq!(
            add_up(&[id(1), id(7)], &listing, &[], &known),
            (800, vec![]),
            "the listing's stat wins over what was known"
        );
        // Of a folder opened in place: nothing here; gone since: left out.
        assert_eq!(
            add_up(&[id(1), id(8), id(9)], &listing, &[id(8)], &known),
            (300, vec![])
        );
        assert_eq!(add_up(&[], &listing, &[], &known), (0, vec![]));
    }

    /// Each name is stat-ed as the listing stats it, kept by the id the
    /// listing gave it: a folder weighs nothing, two names of one file
    /// are that file, a link weighs what it leads to.
    #[test]
    fn a_folders_files_are_stat_ed_as_the_listing_stats_them() {
        let dir = scratch("stat");
        std::fs::write(dir.join("it's a report.txt"), [0u8; 300]).unwrap();
        std::fs::write(dir.join("notes.md"), [0u8; 20]).unwrap();
        std::fs::write(dir.join(".hidden"), [0u8; 5]).unwrap();
        std::fs::create_dir(dir.join("folder")).unwrap();
        std::fs::write(dir.join("folder").join("inside"), [0u8; 1000]).unwrap();
        // Extended with links and hard links where the system makes them.
        #[cfg_attr(not(unix), allow(unused_mut))]
        let mut names: Vec<(EntryId, OsString, EntryKind)> = vec![
            (id(1), "it's a report.txt".into(), EntryKind::File),
            (id(2), "notes.md".into(), EntryKind::File),
            (id(3), ".hidden".into(), EntryKind::File),
            (id(4), "folder".into(), EntryKind::Directory),
            (id(5), "gone since".into(), EntryKind::File),
        ];
        #[cfg(unix)]
        {
            std::fs::hard_link(dir.join("notes.md"), dir.join("notes again.md")).unwrap();
            let away = dir.join("away «iso»");
            std::fs::create_dir(&away).unwrap();
            std::fs::write(away.join("big"), [0u8; 4000]).unwrap();
            std::os::unix::fs::symlink(away.join("big"), dir.join("iso ’link’")).unwrap();
            std::os::unix::fs::symlink(&away, dir.join("to a folder")).unwrap();
            names.extend([
                (id(6), "notes again.md".into(), EntryKind::File),
                (id(7), "iso ’link’".into(), EntryKind::Symlink),
                (id(8), "to a folder".into(), EntryKind::Symlink),
            ]);
        }
        let folder = skwir_core::fs::Dir::open(&dir).unwrap();
        let token = CancelToken::new();
        let known: HashMap<EntryId, Weighed> = stat_names(&folder, &names, &token)
            .unwrap()
            .into_iter()
            .collect();
        assert!(matches!(known[&id(1)], Weighed::File(_, 300)));
        assert!(matches!(known[&id(3)], Weighed::File(_, 5)));
        assert_eq!(known[&id(4)], Weighed::Nothing, "a folder weighs nothing");
        assert!(
            !known.contains_key(&id(5)),
            "not stat-ed: tried again later"
        );
        let total = |ids: &[EntryId]| {
            let mut total = Total::default();
            for id in ids {
                if let Some(weighed) = known.get(id) {
                    total.add(*weighed);
                }
            }
            total.bytes
        };
        assert_eq!(total(&[id(1), id(2), id(4), id(5)]), 320);
        #[cfg(unix)]
        {
            assert_eq!(known[&id(6)], known[&id(2)], "two names, one file");
            assert!(matches!(known[&id(7)], Weighed::File(_, 4000)));
            assert_eq!(known[&id(8)], Weighed::Nothing);
            let all: Vec<EntryId> = names.iter().map(|(id, _, _)| *id).collect();
            assert_eq!(total(&all), 4325);
        }
        token.cancel();
        assert!(stat_names(&folder, &names, &token).is_none());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
