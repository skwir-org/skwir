//! What the Map view shows (a `ViewRole::Lens`, ADR-007 and ADR-020): the
//! files of the location a pane shows that say where they are, pictures by
//! the position their camera wrote and tracks by their line, found on
//! workers and handed to the view a batch at a time.
//!
//! `geo.scan { pane }` starts, or keeps, the scan of what the pane lists:
//! a folder, a search, a timeline's bucket alike. It notes which listing
//! and returns; reading it (a slice at a time, what was looked at left
//! out), choosing the candidates, reading their positions and publishing
//! happen on workers. A scan follows the pane's location lifetime, so
//! navigating away cancels it. What a file says is kept by its identity
//! (device, inode, size, modification time), in memory and in the
//! deletable cache, so a folder seen before is placed at once: a picture's
//! position and clock are fields (`position.rs`), kept by the core with
//! what a search by place reads; a track's line is kept here. A file the
//! listing has stat-ed already is not stat-ed again. `geo.points { pane, since }` hands over what was found after
//! the first `since` points: each point travels once, however many
//! thousand pictures a scan finds.
//!
//! The same listing asked again (a file added, a picture changed) is not
//! a blank start. While its scan runs, the next one begins from what was
//! already looked at, so the markers stay while a large folder streams.
//! Once it has finished, what it found stays shown while the files are
//! looked at again beside it (`Scans::next`), and is replaced at once,
//! under a new generation, only if something moved. A scan resumed keeps
//! the generation of the one before while it took nothing away: the view
//! is handed only what it adds.
use std::collections::{HashMap, HashSet};
use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant, UNIX_EPOCH};

use bibli_core::cache::CacheSlot;
use bibli_core::entry::{Value, keys};
use bibli_core::geo::LatLon;
use bibli_core::location::OpenLocation;
use bibli_core::map_points::{Batch, MapSource, Placed};
use bibli_core::scheduler::Priority;
use bibli_core::workspace::PaneId;
use bibli_core::{App, CancelToken, EntryId};
use parking_lot::Mutex;
use serde::{Deserialize, Serialize};

/// What the tracks kept in the cache may take: each is a small file, a
/// record per version of a file, so the disk holds a block of each.
const CACHE_BYTES: u64 = 8 << 20;
/// Workers one scan is spread over.
const WORKERS: usize = 4;
/// New points are published at most this often while a scan runs.
const PUBLISH: Duration = Duration::from_millis(200);
/// Points of a track's line in the view: its shape at the scale of a
/// folder of trips, not every turn.
const LINE_POINTS: usize = 400;
/// The entries of a listing a scan's worker reads under one hold of the
/// workspace's lock.
const SLICE: usize = 1024;
/// Tracks remembered in memory; beyond, the cache on disk answers.
const REMEMBERED: usize = 200_000;
/// What the cache keeps about a file; a change of `Found` changes it.
const FOUND_VERSION: u32 = 2;

/// What a file says about where it is, kept by its identity.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
struct Found {
    at: Option<[f64; 2]>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    line: Vec<Vec<[f64; 2]>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    clock: Option<u64>,
}

/// A file to look at: its entry, its path, and its size and modification
/// time (in nanoseconds since 1970) when the listing knows them: no link,
/// stat-ed already.
struct Candidate {
    id: EntryId,
    path: Option<PathBuf>,
    known: Option<(u64, u64)>,
}

/// An entry listed and not looked at yet: its name, where a link leads,
/// and what the listing knows of it (`Candidate::known`).
struct Listed {
    id: EntryId,
    name: OsString,
    target: Option<PathBuf>,
    known: Option<(u64, u64)>,
}

/// What was listed: the location, its listing, the versions of its names
/// and of its modification times, and whether it was complete. A scan of
/// a listing still arriving does not end complete: the next one goes on
/// from it.
type Source = (String, u64, u64, u64, bool);

/// A pane's scan.
struct Scan {
    source: Source,
    /// Which scan this is: what its workers find goes to it alone.
    id: u64,
    /// What the view is told: its points are the first ones of the
    /// generation's.
    generation: u64,
    placed: Vec<Placed>,
    /// Files looked at, placed or not, of those that could say where they
    /// are: by the scans it went on from, which it skips (its workers
    /// share the set as they start), and by itself. None is in both.
    before: Arc<HashSet<EntryId>>,
    looked: HashSet<EntryId>,
    candidates: usize,
    listed: usize,
    /// The listing's removals when it started (`Listing::removed_version`):
    /// one that goes on from it with none since keeps all it holds.
    removed: u64,
    complete: bool,
    cancel: CancelToken,
    published: Instant,
}

impl Scan {
    /// How many files it, and the scans it went on from, looked at.
    fn looked_count(&self) -> usize {
        self.before.len() + self.looked.len()
    }

    fn has_looked(&self, id: EntryId) -> bool {
        self.before.contains(&id) || self.looked.contains(&id)
    }
}

/// A pane's scans: the one the view shows, and the one looking at the
/// files again beside it, which replaces it only if something moved.
struct Scans {
    shown: Scan,
    next: Option<Scan>,
}

impl Scans {
    fn latest(&self) -> &Scan {
        self.next.as_ref().unwrap_or(&self.shown)
    }

    fn cancel(&self) {
        self.shown.cancel.cancel();
        if let Some(next) = &self.next {
            next.cancel.cancel();
        }
    }
}

/// How a scan starts, from what the pane's last scan found.
enum Start {
    /// Another listing: nothing of the last scan holds.
    Fresh,
    /// The same listing, its scan not finished: go on from what it looked
    /// at, under its generation when none of its points went.
    Resume(Vec<Placed>, HashSet<EntryId>, Option<u64>),
    /// The same listing, its scan finished: look again beside it.
    Quiet,
}

pub struct Scanner {
    app: std::sync::Weak<App>,
    scans: Mutex<HashMap<PaneId, Scans>>,
    remembered: Mutex<HashMap<(EntryId, u64, u64), Found>>,
    cache: CacheSlot,
    generations: AtomicU64,
    /// The view descriptor's revision: advanced when there is news.
    pub revision: Arc<AtomicU64>,
    wake: bibli_core::location::Waker,
}

/// Whether a name is one this view reads: a picture, or a track.
fn readable(name: &std::ffi::OsStr) -> Option<Kind> {
    if crate::Format::placeable(Path::new(name)) {
        return Some(Kind::Track);
    }
    let mime = bibli_platform::mime::by_name(name)?;
    let is = |kind| bibli_platform::mime::is_kind(&mime, kind);
    if is(bibli_platform::mime::Kind::Image) {
        Some(Kind::Picture)
    } else if is(bibli_platform::mime::Kind::Video) {
        Some(Kind::Video)
    } else {
        None
    }
}

#[derive(Clone, Copy, PartialEq)]
enum Kind {
    Picture,
    /// Where a phone recorded it (`bibli_platform::mp4`).
    Video,
    Track,
}

/// A track's line and start, thinned for the view.
fn track(path: &Path, cancel: &CancelToken) -> Found {
    let Ok(data) = crate::inspect_file(path, cancel) else {
        return Found {
            at: None,
            line: Vec::new(),
            clock: None,
        };
    };
    let Some(doc) = data.geo else {
        return Found {
            at: None,
            line: Vec::new(),
            clock: None,
        };
    };
    let total: usize = doc
        .lines
        .iter()
        .flat_map(|l| &l.segments)
        .map(Vec::len)
        .sum();
    let step = total.div_ceil(LINE_POINTS).max(1);
    let line: Vec<Vec<[f64; 2]>> = doc
        .lines
        .iter()
        .flat_map(|l| &l.segments)
        .map(|segment| {
            let mut kept: Vec<[f64; 2]> = segment
                .iter()
                .step_by(step)
                .map(|p| [p.at.lat, p.at.lon])
                .collect();
            if let Some(last) = segment.last()
                && kept.last() != Some(&[last.at.lat, last.at.lon])
            {
                kept.push([last.at.lat, last.at.lon]);
            }
            kept
        })
        .filter(|segment| !segment.is_empty())
        .collect();
    let at = line
        .first()
        .and_then(|s| s.first())
        .copied()
        .or_else(|| doc.waypoints.first().map(|w| [w.at.lat, w.at.lon]));
    Found {
        at,
        line,
        clock: None,
    }
}

/// What `pane`'s listing `listing` lists that `skip` does not hold, read
/// a slice at a time, each under a hold of the lock of its own: a folder
/// streaming is scanned again at each slice, which read every entry on the
/// window's thread. A link is read, and typed, as what it leads to; a
/// folder, a pipe, a device or a link that leads nowhere has nothing to
/// read. Another listing in the pane by now has a scan of its own.
fn to_look_at(
    app: &App,
    pane: PaneId,
    listing: u64,
    skip: &HashSet<EntryId>,
    cancel: &CancelToken,
) -> Vec<Listed> {
    let mut entries = Vec::new();
    let mut at = 0;
    while !cancel.is_cancelled() {
        let ws = app.workspace.read();
        let Some(p) = ws.pane(pane).filter(|p| p.listing.id() == listing) else {
            break;
        };
        let end = (at + SLICE).min(p.listing.len());
        if at >= end {
            break;
        }
        entries.extend(
            (at..end)
                .filter_map(|i| p.listing.get_index(i))
                .filter(|entry| entry.inode_type().is_none() && !skip.contains(&entry.id))
                .map(|entry| {
                    let target = entry.target.as_ref().and_then(|t| t.path.clone());
                    let size = entry.metadata.value(keys::SIZE).and_then(Value::as_u64);
                    let modified = entry
                        .metadata
                        .value(keys::MODIFIED)
                        .and_then(Value::as_time)
                        .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
                        .map(|d| u64::try_from(d.as_nanos()).unwrap_or(u64::MAX));
                    let known = size
                        .zip(modified)
                        .filter(|_| entry.kind == bibli_core::EntryKind::File);
                    Listed {
                        id: entry.id,
                        name: entry.name.clone(),
                        target,
                        known,
                    }
                }),
        );
        at = end;
    }
    entries
}

/// What a pane's Map view draws, read where it is kept.
impl MapSource for Scanner {
    fn points(&self, pane: PaneId, since: usize, withheld: &HashSet<EntryId>) -> Option<Batch> {
        self.batch(pane, since, withheld)
    }
}

impl Scanner {
    pub fn new(app: &App, revision: Arc<AtomicU64>) -> Scanner {
        Scanner {
            app: app.weak(),
            scans: Mutex::default(),
            remembered: Mutex::default(),
            cache: app.cache.namespace("geo"),
            generations: AtomicU64::new(1),
            revision,
            wake: app.waker_handle(),
        }
    }

    fn changed(&self) {
        self.revision.fetch_add(1, Ordering::Release);
        (self.wake)();
    }

    /// Scan what `pane` lists, unless it is being scanned already.
    pub fn scan(self: &Arc<Self>, app: &App, pane: PaneId) -> anyhow::Result<()> {
        let (source, open, dir, lifetime, listed, removed, start) = {
            let ws = app.workspace.read();
            let pane_ref = ws
                .pane(pane)
                .ok_or_else(|| anyhow::anyhow!("no pane {}", pane.0))?;
            let location = pane_ref
                .location()
                .map(ToString::to_string)
                .unwrap_or_default();
            let listing = &pane_ref.listing;
            let source = (
                location,
                listing.id(),
                // Files new, gone or written again, not first stat-ed:
                // the scan stats each file itself.
                listing.names_version(),
                listing.rewritten_version(),
                listing.is_complete(),
            );
            let start = match self.scans.lock().get_mut(&pane) {
                Some(scans) if scans.latest().cancel.is_cancelled() => Start::Fresh,
                Some(scans) if scans.latest().source == source => return Ok(()),
                Some(Scans { shown, .. })
                    if (&shown.source.0, shown.source.1) == (&source.0, source.1) =>
                {
                    if shown.complete {
                        Start::Quiet
                    } else {
                        // What is still listed of what was looked at, taken
                        // from the scan this one replaces below: copied,
                        // every point found was copied at each slice of a
                        // folder streaming. A folder streaming only gains
                        // entries: with nothing taken out since, all of it
                        // is, and it is not walked at each slice.
                        let gone = shown.removed != listing.removed_version();
                        let mut placed = std::mem::take(&mut shown.placed);
                        let before = placed.len();
                        if gone {
                            placed.retain(|p| listing.get(p.id).is_some());
                        }
                        let kept = (placed.len() == before).then_some(shown.generation);
                        // Held by no worker by now (they read it as they
                        // start), so not copied.
                        let before = std::mem::take(&mut shown.before);
                        let mut looked =
                            Arc::try_unwrap(before).unwrap_or_else(|shared| (*shared).clone());
                        looked.extend(shown.looked.drain());
                        if gone {
                            looked.retain(|&id| listing.get(id).is_some());
                        }
                        Start::Resume(placed, looked, kept)
                    }
                }
                _ => Start::Fresh,
            };
            (
                source,
                pane_ref.open.clone(),
                pane_ref.location().and_then(bibli_core::Location::path),
                pane_ref.location_lifetime(),
                listing.len(),
                listing.removed_version(),
                start,
            )
        };
        let cancel = app.lifetime().child().linked(&lifetime);
        let id = self.generations.fetch_add(1, Ordering::Relaxed);
        let generation = match &start {
            Start::Resume(_, _, Some(kept)) => *kept,
            _ => id,
        };
        tracing::debug!(
            pane = pane.0,
            id,
            generation,
            start = match &start {
                Start::Fresh => "fresh",
                Start::Resume(..) => "resume",
                Start::Quiet => "quiet",
            },
            "geo: scan"
        );
        let (placed, looked, quiet) = match start {
            Start::Fresh => (Vec::new(), HashSet::new(), false),
            Start::Resume(placed, looked, _) => (placed, looked, false),
            Start::Quiet => (Vec::new(), HashSet::new(), true),
        };
        let skip: Arc<HashSet<EntryId>> = Arc::new(looked);
        let listing = source.1;
        let scan = Scan {
            source,
            id,
            generation,
            placed,
            before: Arc::clone(&skip),
            looked: HashSet::new(),
            candidates: 0,
            listed,
            removed,
            complete: false,
            cancel: cancel.clone(),
            published: Instant::now(),
        };
        {
            let mut scans = self.scans.lock();
            if quiet && let Some(scans) = scans.get_mut(&pane) {
                if let Some(old) = scans.next.replace(scan) {
                    old.cancel.cancel();
                }
            } else if let Some(old) = scans.insert(
                pane,
                Scans {
                    shown: scan,
                    next: None,
                },
            ) {
                old.cancel();
            }
        }
        if !quiet {
            self.changed();
        }
        // Which files can say where they are is for a worker to find: what
        // is listed and not looked at yet, the MIME database asked by name,
        // and a search's entries for their path.
        let this = Arc::clone(self);
        let token = cancel.clone();
        app.scheduler
            .spawn(Priority::Normal, "geo-candidates", cancel, move || {
                let Some(app) = this.app.upgrade() else {
                    return;
                };
                let entries = to_look_at(&app, pane, listing, &skip, &token);
                this.start(
                    pane,
                    id,
                    entries,
                    &skip,
                    open.as_deref(),
                    dir.as_deref(),
                    &token,
                );
            });
        Ok(())
    }

    #[allow(clippy::too_many_arguments)] // one scan's inputs, from `scan`
    fn start(
        self: &Arc<Self>,
        pane: PaneId,
        scan_id: u64,
        entries: Vec<Listed>,
        skip: &HashSet<EntryId>,
        open: Option<&dyn OpenLocation>,
        dir: Option<&Path>,
        cancel: &CancelToken,
    ) {
        let candidates: Vec<(Candidate, Kind)> = entries
            .into_iter()
            .filter_map(|listed| {
                let Listed {
                    id,
                    name,
                    target,
                    known,
                } = listed;
                let kind = readable(target.as_deref().and_then(Path::file_name).unwrap_or(&name))?;
                let path = target.or_else(|| match &dir {
                    Some(dir) => Some(dir.join(&name)),
                    None => open.and_then(|o| o.path_of(id)),
                });
                Some((Candidate { id, path, known }, kind))
            })
            .collect();
        let count = candidates.len();
        let Some(shown) = self.update(pane, scan_id, |scan| scan.candidates = count) else {
            return;
        };
        let candidates: Arc<Vec<(Candidate, Kind)>> = Arc::new(
            candidates
                .into_iter()
                .filter(|(candidate, _)| !skip.contains(&candidate.id))
                .collect(),
        );
        if shown {
            self.changed();
        }
        if candidates.is_empty() {
            self.finish(pane, scan_id);
            return;
        }
        let left = Arc::new(AtomicU64::new(WORKERS as u64));
        let Some(app) = self.app.upgrade() else {
            return;
        };
        for worker in 0..WORKERS {
            let (this, candidates, left, token) = (
                Arc::clone(self),
                Arc::clone(&candidates),
                Arc::clone(&left),
                cancel.clone(),
            );
            app.scheduler.spawn(
                Priority::Normal,
                "geo-positions",
                cancel.clone(),
                move || {
                    for (candidate, kind) in candidates.iter().skip(worker).step_by(WORKERS) {
                        if token.is_cancelled() {
                            return;
                        }
                        let found = this.find(candidate, *kind, &token);
                        let placed = found.and_then(|found| {
                            let [lat, lon] = found.at?;
                            LatLon::new(lat, lon)?;
                            Some(Placed {
                                id: candidate.id,
                                lat,
                                lon,
                                line: found.line,
                                clock: found.clock,
                            })
                        });
                        let mut publish = false;
                        let Some(shown) = this.update(pane, scan_id, |scan| {
                            scan.looked.insert(candidate.id);
                            scan.placed.extend(placed);
                            if scan.published.elapsed() >= PUBLISH {
                                scan.published = Instant::now();
                                publish = true;
                            }
                        }) else {
                            return;
                        };
                        if publish && shown {
                            this.changed();
                        }
                    }
                    if left.fetch_sub(1, Ordering::AcqRel) == 1 {
                        this.finish(pane, scan_id);
                    }
                },
            );
        }
    }

    /// Change the scan of `pane` if it is still scan `id`: whether it
    /// is the one shown, or `None` when another scan replaced it.
    fn update(&self, pane: PaneId, id: u64, change: impl FnOnce(&mut Scan)) -> Option<bool> {
        let mut scans = self.scans.lock();
        let scans = scans.get_mut(&pane)?;
        if scans.shown.id == id {
            change(&mut scans.shown);
            return Some(true);
        }
        let next = scans.next.as_mut().filter(|n| n.id == id)?;
        change(next);
        Some(false)
    }

    /// The scan `id` of `pane` ended. One that looked again beside
    /// the one shown replaces it if something moved, and otherwise goes
    /// without a word.
    fn finish(&self, pane: PaneId, id: u64) {
        let news = {
            let mut scans = self.scans.lock();
            let Some(scans) = scans.get_mut(&pane) else {
                return;
            };
            if scans.shown.id == id {
                scans.shown.complete = scans.shown.source.4;
                true
            } else if let Some(mut next) = scans.next.take_if(|n| n.id == id) {
                next.complete = next.source.4;
                if same_places(&scans.shown, &next) {
                    scans.shown.source = next.source;
                    scans.shown.listed = next.listed;
                    false
                } else {
                    scans.shown = next;
                    true
                }
            } else {
                false
            }
        };
        tracing::debug!(pane = pane.0, id, news, "geo: scan finished");
        if news {
            self.changed();
        }
    }

    /// What `candidate` says about where it is: remembered, cached, or read.
    fn find(&self, candidate: &Candidate, kind: Kind, cancel: &CancelToken) -> Option<Found> {
        let path = candidate.path.as_deref()?;
        if kind != Kind::Track {
            // A picture's or a video's position and clock are fields, read
            // together and kept by the core (`position.rs`).
            let app = self.app.upgrade()?;
            let known = candidate.known.map(|(len, modified)| {
                (candidate.id.file.dev, candidate.id.file.ino, len, modified)
            });
            let mut said = app
                .fields
                .get_all(
                    &[keys::POSITION, crate::position::CLOCK],
                    path,
                    known,
                    cancel,
                )
                .into_iter();
            let (at, clock) = (said.next().flatten(), said.next().flatten());
            if cancel.is_cancelled() {
                return None;
            }
            return Some(Found {
                at: at
                    .as_ref()
                    .and_then(LatLon::from_value)
                    .map(|at| [at.lat, at.lon]),
                line: Vec::new(),
                clock: clock.as_ref().and_then(Value::as_u64),
            });
        }
        let identity = if let Some((len, modified)) = candidate.known {
            (candidate.id, len, modified)
        } else {
            let metadata = std::fs::metadata(path).ok()?;
            let modified = metadata
                .modified()
                .ok()
                .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
                .map_or(0, |d| u64::try_from(d.as_nanos()).unwrap_or(u64::MAX));
            (candidate.id, metadata.len(), modified)
        };
        if let Some(found) = self.remembered.lock().get(&identity) {
            return Some(found.clone());
        }
        // Versioned: what an older bibli kept did not have the clock.
        let key = format!(
            "{FOUND_VERSION}:{}:{}:{}:{}",
            candidate.id.file.dev, candidate.id.file.ino, identity.1, identity.2
        );
        let found = self
            .cache
            .get(key.as_bytes())
            .and_then(|bytes| serde_json::from_slice::<Found>(&bytes).ok())
            .unwrap_or_else(|| {
                let found = track(path, cancel);
                if !cancel.is_cancelled()
                    && let Ok(bytes) = serde_json::to_vec(&found)
                {
                    match self.cache.put(key.as_bytes(), &bytes) {
                        // Bounded: a record per version of every file
                        // ever looked at, else.
                        Ok(()) => self.cache.trim_soon(bytes.len() as u64, CACHE_BYTES),
                        Err(error) => tracing::debug!(%error, "geo: keeping a track"),
                    }
                }
                found
            });
        // A read cut short says nothing of the file: remembered, a track
        // cancelled mid-parse would have no position for the session.
        if cancel.is_cancelled() {
            return None;
        }
        let mut remembered = self.remembered.lock();
        if remembered.len() >= REMEMBERED {
            remembered.clear();
        }
        remembered.insert(identity, found.clone());
        Some(found)
    }

    /// What the scan of `pane` found after its first `since` points.
    ///
    /// Without the files `withheld` (an agent's call, `App::withheld`), as
    /// if the pane did not list them: their places and their counts.
    pub fn batch(&self, pane: PaneId, since: usize, withheld: &HashSet<EntryId>) -> Option<Batch> {
        let scans = self.scans.lock();
        let scans = scans.get(&pane)?;
        let scan = &scans.shown;
        let placed: Vec<&Placed> = scan
            .placed
            .iter()
            .filter(|p| !withheld.contains(&p.id))
            .collect();
        let looked = scan
            .before
            .iter()
            .chain(&scan.looked)
            .filter(|id| withheld.contains(id))
            .count();
        Some(Batch {
            generation: scan.generation,
            points: placed
                .get(since..)
                .unwrap_or_default()
                .iter()
                .map(|p| (*p).clone())
                .collect(),
            placed: placed.len(),
            looked: scan.looked_count() - looked,
            candidates: scan.candidates.saturating_sub(looked),
            listed: scan.listed.saturating_sub(withheld.len()),
            complete: scan.complete,
            rechecking: scans.next.is_some(),
        })
    }

    /// Forget the scan of `pane`: the view is closed.
    pub fn release(&self, pane: PaneId) {
        if let Some(scans) = self.scans.lock().remove(&pane) {
            scans.cancel();
        }
    }

    /// Stop everything: the plugin is disabled.
    pub fn stop(&self) {
        for (_, scans) in self.scans.lock().drain() {
            scans.cancel();
        }
        self.remembered.lock().clear();
        self.changed();
    }
}

/// Whether two scans placed the same files at the same places, and looked
/// at as many.
fn same_places(a: &Scan, b: &Scan) -> bool {
    if a.placed.len() != b.placed.len()
        || a.looked_count() != b.looked_count()
        || !a.before.iter().chain(&a.looked).all(|id| b.has_looked(*id))
    {
        return false;
    }
    let before: HashMap<EntryId, &Placed> = a.placed.iter().map(|p| (p.id, p)).collect();
    b.placed.iter().all(|p| before.get(&p.id) == Some(&p))
}
