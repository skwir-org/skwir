//! The thumbnail service.
//!
//! Providers (registered by the platform crate and plugins) produce decoded
//! RGBA images for an entry: a cache reader, an image decoder, external
//! thumbnailers, and so on. The service tries them in the order of their
//! [`Stage`] on a worker, most-recently-requested first, drops requests the
//! viewport scrolled away from, keeps a byte-budgeted LRU of results, and
//! wakes the UI when one lands. Toolkit-specific GPU upload is the UI's job.
//!
//! A result is about one version of a file: it is keyed by the entry, the
//! size and the modification time the request carries, so an edited file,
//! or one that was still being written when it was first asked for, is a
//! new request rather than the old picture or the old failure.

use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::time::SystemTime;

use parking_lot::{Mutex, RwLock};
use serde::{Deserialize, Serialize};

use crate::cancel::CancelToken;
use crate::entry::EntryId;
use crate::location::Waker;
use crate::lru::ByteLru;
use crate::op::EntryRef;
use crate::scheduler::{Priority, Scheduler};

/// A decoded RGBA8 image, straight rows, no padding.
#[derive(Clone, Debug)]
pub struct Image {
    pub width: u32,
    pub height: u32,
    pub rgba: Arc<[u8]>,
}

impl Image {
    pub fn bytes(&self) -> usize {
        self.rgba.len()
    }
}

/// Thumbnail sizes, matching the freedesktop spec's directories.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ThumbSize {
    /// Up to 128 px.
    Normal,
    /// Up to 256 px.
    Large,
}

impl ThumbSize {
    pub fn pixels(self) -> u32 {
        match self {
            ThumbSize::Normal => 128,
            ThumbSize::Large => 256,
        }
    }
}

/// Everything a provider needs to find or make a thumbnail.
#[derive(Clone, Debug)]
pub struct ThumbRequest {
    pub entry: EntryRef,
    /// MIME type if known (`image/png`), so providers can skip cheaply.
    pub mime: Option<Arc<str>>,
    /// Modification time, for cache freshness.
    pub mtime: Option<SystemTime>,
    pub size: ThumbSize,
}

/// Where a provider stands in the order they are tried. Providers of one
/// stage keep their registration order; stages do not depend on which plugin
/// registered first, or re-registered after a settings change.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Stage {
    /// Reads what exists (the desktop's cache) and stores what later
    /// providers make (`ThumbnailProvider::store`).
    Cache,
    /// Makes a thumbnail: in-process decoders, the desktop's thumbnailers.
    Generate,
    /// Makes one only for what nothing the desktop installed could: our own
    /// renderers, so an installed thumbnailer keeps the look other
    /// applications share.
    Fallback,
    /// A converter of many formats (`ImageMagick`, ADR-031), asked only when
    /// nothing made for the format itself could.
    LastResort,
}

/// A thumbnail source. Tried by [`Stage`], then in registration order.
pub trait ThumbnailProvider: Send + Sync {
    fn id(&self) -> &'static str;
    fn stage(&self) -> Stage {
        Stage::Generate
    }
    /// Cheap pre-check so the queue is not filled with hopeless work.
    fn accepts(&self, req: &ThumbRequest) -> bool;
    /// Find or make the thumbnail. `Ok(None)` means "not from me, try the next".
    fn thumbnail(&self, req: &ThumbRequest, cancel: &CancelToken) -> anyhow::Result<Option<Image>>;
    /// Persist a thumbnail another provider produced (cache writers).
    fn store(&self, _req: &ThumbRequest, _image: &Image) {}
    /// Whether it remembers that no provider could make a thumbnail of
    /// `req`'s file as it is now, with the tools there are now (a cache's
    /// failures, the desktop's `fail/` folder): none is tried again.
    fn failed_before(&self, _req: &ThumbRequest) -> bool {
        false
    }
    /// Remember that no provider could make one of `req`'s file as it is.
    fn store_failure(&self, _req: &ThumbRequest) {}
}

/// What a provider answers, as its error, when it cannot make the
/// thumbnail now without keeping a worker waiting: a program it runs only
/// so many at once (`ImageMagick`), all running. The request is set
/// aside, the worker goes on with others, and it is taken again, from
/// that provider on, once `Busy` says it can run.
pub struct Busy {
    when_free: Mutex<Option<WhenFree>>,
}

/// Given what to call once the provider can run.
type WhenFree = Box<dyn FnOnce(Box<dyn FnOnce() + Send>) + Send>;

impl Busy {
    /// `when_free` is handed what to call once the provider can run: at
    /// once if it can already.
    pub fn new(when_free: impl FnOnce(Box<dyn FnOnce() + Send>) + Send + 'static) -> Self {
        Busy {
            when_free: Mutex::new(Some(Box::new(when_free))),
        }
    }

    fn when_free(&self, then: Box<dyn FnOnce() + Send>) {
        match self.when_free.lock().take() {
            Some(when_free) => when_free(then),
            None => then(),
        }
    }
}

impl std::fmt::Debug for Busy {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Busy")
    }
}

impl std::fmt::Display for Busy {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("waiting for a turn")
    }
}

impl std::error::Error for Busy {}

/// What the UI gets back synchronously.
#[derive(Clone, Debug)]
pub enum ThumbState {
    Ready(Arc<Image>),
    /// Queued or in progress; the waker fires when it lands.
    Pending,
    /// No provider could produce one.
    Unavailable,
}

/// An entry's picture at one size, as of one modification time.
type Key = (EntryId, ThumbSize, Option<SystemTime>);

/// What waits for a worker: how much it is wanted is its [`Flight`]'s,
/// which a frame asking again updates in place.
struct Queued {
    key: Key,
    req: ThumbRequest,
    seq: u64,
    /// Where it goes on from once set aside ([`Busy`]): the provider that
    /// was busy, and whether one that makes thumbnails tried before it.
    /// What ran before is not asked again.
    resume: Option<Resume>,
}

/// A request asked for and not answered yet, queued, set aside or
/// running.
struct Flight {
    /// The frame it was last asked for: older than a few frames, the
    /// viewport has moved on and it is dropped before it runs, or stopped
    /// while it runs.
    asked: u64,
    /// The best priority it was asked with.
    priority: Priority,
    cancel: CancelToken,
}

#[derive(Clone, Copy)]
struct Resume {
    provider: usize,
    /// The providers' version it was set aside under: another set starts
    /// over.
    providers: u64,
    tried: bool,
}

struct State {
    lru: ByteLru<Key, Arc<Image>>,
    unavailable: HashSet<Key>,
    /// Pending requests; workers take the best priority, most recent first.
    queue: Vec<Queued>,
    /// Requests set aside while their provider is busy ([`Busy`]), queued
    /// again once it can run.
    parked: Vec<Queued>,
    /// Every request asked for and not answered, by key: a frame asking
    /// again finds it here, never by a walk of the queues.
    in_flight: HashMap<Key, Flight>,
    /// Bumped when the providers change: a request that ran against an
    /// older set does not record the file as unavailable.
    providers: u64,
    frame: u64,
    seq: u64,
    decoded: u64,
    skipped: u64,
    /// Workers taking requests now (`RUNNING_AT_MOST`).
    running: usize,
    /// The memory the thumbnails shown may take, as set: what the UI
    /// keeps of what it draws. This keeps a quarter of it, what was made
    /// and not taken yet.
    budget: usize,
}

/// Frames a request may go unasked before it is dropped or cancelled.
const STALE_FRAMES: u64 = 3;

/// The service. One per app, shared with workers.
pub struct Thumbnails {
    /// Owner, stage and provider, sorted by stage.
    providers: RwLock<Vec<(Arc<str>, Stage, Arc<dyn ThumbnailProvider>)>>,
    state: Mutex<State>,
    scheduler: Scheduler,
    waker: Mutex<Option<Waker>>,
    /// Stops every queued and running request when the owner goes away.
    lifetime: CancelToken,
}

impl Thumbnails {
    /// A service whose work lives as long as the returned value is used.
    pub fn new(scheduler: Scheduler) -> Arc<Self> {
        Self::with_lifetime(scheduler, CancelToken::new())
    }

    /// A service whose providers are cancelled when `lifetime` is (the app's).
    pub fn with_lifetime(scheduler: Scheduler, lifetime: CancelToken) -> Arc<Self> {
        Arc::new(Thumbnails {
            providers: RwLock::new(Vec::new()),
            state: Mutex::new(State {
                lru: ByteLru::new(16 << 20),
                unavailable: HashSet::new(),
                queue: Vec::new(),
                parked: Vec::new(),
                in_flight: HashMap::new(),
                providers: 0,
                frame: 0,
                seq: 0,
                decoded: 0,
                skipped: 0,
                running: 0,
                budget: 64 << 20,
            }),
            scheduler,
            waker: Mutex::new(None),
            lifetime,
        })
    }

    pub fn set_waker(&self, waker: Option<Waker>) {
        *self.waker.lock() = waker;
    }

    /// Bytes of decoded images the thumbnails shown may take, as the user
    /// set it (64 MiB by default). Whoever draws them keeps them
    /// (`budget`); this keeps a quarter of it, of those made and not taken
    /// yet, so the same pixels are not held twice in full.
    pub fn set_budget(&self, bytes: usize) {
        let evicted = {
            let mut s = self.state.lock();
            s.budget = bytes;
            s.lru.set_budget(bytes / 4)
        };
        drop(evicted);
    }

    /// The memory thumbnails drawn may take, as set (`set_budget`).
    pub fn budget(&self) -> usize {
        self.state.lock().budget
    }

    pub fn register(&self, owner: &str, provider: Arc<dyn ThumbnailProvider>) {
        let stage = provider.stage();
        {
            let mut providers = self.providers.write();
            let at = providers.partition_point(|(_, s, _)| *s <= stage);
            providers.insert(at, (owner.into(), stage, provider));
        }
        self.providers_changed();
    }

    pub fn unregister_owner(&self, owner: &str) {
        self.providers.write().retain(|(o, _, _)| &**o != owner);
    }

    /// A provider can now make what it could not (it found its tools, say):
    /// forget which files had no thumbnail, and wake the UI to ask again.
    pub fn providers_changed(&self) {
        {
            let mut s = self.state.lock();
            s.unavailable.clear();
            s.providers += 1;
        }
        self.wake();
    }

    /// Something a provider reads now exists for `id` (the preview converted
    /// it, say): forget that it had no thumbnail, and wake the UI to ask
    /// again if it had none.
    pub fn retry(&self, id: EntryId) {
        let forgotten = {
            let mut s = self.state.lock();
            let before = s.unavailable.len();
            s.unavailable.retain(|(entry, _, _)| *entry != id);
            s.unavailable.len() != before
        };
        if forgotten {
            self.wake();
        }
    }

    pub fn provider_ids(&self) -> Vec<&'static str> {
        self.providers
            .read()
            .iter()
            .map(|(_, _, p)| p.id())
            .collect()
    }

    /// Call once per rendered frame: requests not asked for in the last few
    /// frames are dropped before they run, and running ones are cancelled,
    /// since the viewport moved on.
    pub fn tick(&self) {
        let mut s = self.state.lock();
        s.frame += 1;
        let frame = s.frame;
        for flight in s.in_flight.values() {
            if frame.saturating_sub(flight.asked) > STALE_FRAMES {
                flight.cancel.cancel();
            }
        }
    }

    /// Ask for a thumbnail. Returns immediately.
    pub fn get(self: &Arc<Self>, req: ThumbRequest, priority: Priority) -> ThumbState {
        let key = (req.entry.id, req.size, req.mtime);
        {
            let mut s = self.state.lock();
            if let Some(img) = s.lru.get(&key) {
                return ThumbState::Ready(Arc::clone(img));
            }
            if s.unavailable.contains(&key) {
                return ThumbState::Unavailable;
            }
            let frame = s.frame;
            if let Some(flight) = s.in_flight.get_mut(&key) {
                flight.asked = frame;
                flight.priority = flight.priority.min(priority);
                return ThumbState::Pending;
            }
        }
        // Plugin code runs outside the state lock.
        let accepted = {
            let providers = self.providers.read();
            providers.iter().any(|(_, _, p)| p.accepts(&req))
        };
        let mut s = self.state.lock();
        if !accepted {
            s.unavailable.insert(key);
            return ThumbState::Unavailable;
        }
        if s.in_flight.contains_key(&key) || s.lru.contains(&key) {
            // Another caller queued it (or it landed) while we asked.
            return ThumbState::Pending;
        }
        let frame = s.frame;
        s.in_flight.insert(
            key,
            Flight {
                asked: frame,
                priority,
                cancel: self.lifetime.child(),
            },
        );
        s.seq += 1;
        let seq = s.seq;
        s.queue.push(Queued {
            key,
            req,
            seq,
            resume: None,
        });
        drop(s);
        self.spawn_worker(priority);
        ThumbState::Pending
    }

    /// (decoded, skipped as stale, cached bytes)
    pub fn stats(&self) -> (u64, u64, usize) {
        let s = self.state.lock();
        (s.decoded, s.skipped, s.lru.bytes())
    }

    /// One more worker taking requests, unless as many run as may: a
    /// thumbnailer can hold one for seconds (a video, a document), and a
    /// grid of them must leave some to list folders and read previews.
    fn spawn_worker(self: &Arc<Self>, priority: Priority) {
        {
            let mut s = self.state.lock();
            if s.running >= self.running_at_most() {
                return;
            }
            s.running += 1;
        }
        let this = Arc::clone(self);
        self.scheduler
            .spawn(priority, "thumb", self.lifetime.child(), move || {
                // Every request queued meanwhile, one after the other.
                while this.work_one() {}
            });
    }

    /// Workers taking requests at most: all but one, at least one.
    fn running_at_most(&self) -> usize {
        self.scheduler.workers().saturating_sub(1).max(1)
    }

    /// Queue again what was set aside for a busy provider, which can run
    /// now, and have workers take it.
    fn unpark(self: &Arc<Self>) {
        let priorities: Vec<Priority> = {
            let mut s = self.state.lock();
            let parked = std::mem::take(&mut s.parked);
            let priorities = parked
                .iter()
                .filter_map(|q| s.in_flight.get(&q.key).map(|f| f.priority))
                .collect();
            s.queue.extend(parked);
            priorities
        };
        for priority in priorities {
            self.spawn_worker(priority);
        }
    }

    /// Take the best live request (visible first, most recent first) and run
    /// the providers on it. False once nothing is queued: the worker stops
    /// taking requests.
    fn work_one(self: &Arc<Self>) -> bool {
        let (key, req, cancel, generation, seq, resume) = {
            let mut s = self.state.lock();
            // The requests no frame asked for lately, or cancelled, go in
            // one pass: dropped one at a time, each after a look for the
            // best, a fast scroll's hundreds were quadratic under the lock.
            let State {
                queue,
                in_flight,
                skipped,
                frame,
                ..
            } = &mut *s;
            queue.retain(|q| {
                let live = in_flight.get(&q.key).is_some_and(|f| {
                    frame.saturating_sub(f.asked) <= STALE_FRAMES && !f.cancel.is_cancelled()
                });
                if !live {
                    in_flight.remove(&q.key);
                    *skipped += 1;
                }
                live
            });
            // Every request left in the queue is in flight: the pass above
            // dropped the others.
            let best = queue
                .iter()
                .enumerate()
                .min_by_key(|(_, q)| (in_flight[&q.key].priority, std::cmp::Reverse(q.seq)))
                .map(|(i, _)| i);
            let Some(i) = best else {
                s.running = s.running.saturating_sub(1);
                return false;
            };
            let q = s.queue.swap_remove(i);
            let Some(cancel) = s.in_flight.get(&q.key).map(|f| f.cancel.clone()) else {
                return true;
            };
            let resume = q.resume.filter(|r| r.providers == s.providers);
            (q.key, q.req, cancel, s.providers, q.seq, resume)
        };
        let providers: Vec<Arc<dyn ThumbnailProvider>> = self
            .providers
            .read()
            .iter()
            .map(|(_, _, p)| Arc::clone(p))
            .collect();
        let mut produced: Option<(usize, Image)> = None;
        // Remembered as one nothing could draw, as it is and with the tools
        // there are: no provider is run again for it.
        let failed_before = resume.is_none() && providers.iter().any(|p| p.failed_before(&req));
        // Whether anything that makes thumbnails tried: a file only a cache
        // takes (a text file) failed nothing worth remembering.
        let mut tried = resume.is_some_and(|r| r.tried);
        let mut busy = None;
        let from = resume.map_or(0, |r| r.provider);
        for (i, p) in providers.iter().enumerate().skip(from) {
            if cancel.is_cancelled() || failed_before {
                break;
            }
            if !p.accepts(&req) {
                continue;
            }
            match p.thumbnail(&req, &cancel) {
                Ok(Some(img)) => {
                    produced = Some((i, img));
                    break;
                }
                Ok(None) => {}
                Err(e) => match e.downcast::<Busy>() {
                    Ok(b) => {
                        busy = Some((i, b));
                        break;
                    }
                    Err(e) => {
                        tracing::debug!(provider = p.id(), path = %req.entry.path.display(), "thumbnail failed: {e:#}");
                    }
                },
            }
            tried |= p.stage() != Stage::Cache;
        }
        let cancelled = cancel.is_cancelled();
        if let Some((provider, busy)) = busy
            && !cancelled
        {
            self.state.lock().parked.push(Queued {
                key,
                req,
                seq,
                resume: Some(Resume {
                    provider,
                    providers: generation,
                    tried,
                }),
            });
            let this = Arc::downgrade(self);
            busy.when_free(Box::new(move || {
                if let Some(this) = this.upgrade() {
                    this.unpark();
                }
            }));
            return true;
        }
        if let Some((i, img)) = &produced
            && !cancelled
        {
            for p in &providers[..*i] {
                p.store(&req, img);
            }
        }
        let evicted = {
            let mut s = self.state.lock();
            s.in_flight.remove(&key);
            match produced {
                Some((_, img)) => {
                    s.decoded += 1;
                    let bytes = img.bytes();
                    s.lru.insert(key, Arc::new(img), bytes)
                }
                // A cancelled request says nothing about the file: the next
                // ask queues it again.
                None if cancelled => {
                    s.skipped += 1;
                    Vec::new()
                }
                // Providers changed while it ran: the next ask tries the
                // new set.
                None if s.providers != generation => Vec::new(),
                None => {
                    s.unavailable.insert(key);
                    if tried {
                        drop(s);
                        for p in &providers {
                            p.store_failure(&req);
                        }
                    }
                    Vec::new()
                }
            }
        };
        drop(evicted);
        self.wake();
        true
    }

    fn wake(&self) {
        if let Some(w) = self.waker.lock().as_ref() {
            w();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
    use std::time::Duration;

    struct Fake {
        id: &'static str,
        stage: Stage,
        calls: AtomicUsize,
        produce: AtomicBool,
        stored: AtomicUsize,
        /// Failures it was told to remember (a cache only).
        failures: AtomicUsize,
        /// Called while it runs, before it answers.
        during: Mutex<Option<Box<dyn FnOnce() + Send>>>,
    }

    impl Fake {
        fn new(id: &'static str, stage: Stage, produce: bool) -> Arc<Self> {
            Arc::new(Fake {
                id,
                stage,
                calls: AtomicUsize::new(0),
                produce: AtomicBool::new(produce),
                stored: AtomicUsize::new(0),
                failures: AtomicUsize::new(0),
                during: Mutex::new(None),
            })
        }
    }

    impl ThumbnailProvider for Fake {
        fn id(&self) -> &'static str {
            self.id
        }
        fn stage(&self) -> Stage {
            self.stage
        }
        fn accepts(&self, _: &ThumbRequest) -> bool {
            true
        }
        fn thumbnail(&self, _: &ThumbRequest, _: &CancelToken) -> anyhow::Result<Option<Image>> {
            self.calls.fetch_add(1, Ordering::Relaxed);
            let produce = self.produce.load(Ordering::Relaxed);
            if let Some(during) = self.during.lock().take() {
                during();
            }
            Ok(produce.then(|| Image {
                width: 2,
                height: 2,
                rgba: Arc::from(vec![0u8; 16]),
            }))
        }
        fn store(&self, _: &ThumbRequest, _: &Image) {
            self.stored.fetch_add(1, Ordering::Relaxed);
        }
        // A cache remembers failures, as the desktop's does.
        fn failed_before(&self, _: &ThumbRequest) -> bool {
            self.stage == Stage::Cache && self.failures.load(Ordering::Relaxed) > 0
        }
        fn store_failure(&self, _: &ThumbRequest) {
            if self.stage == Stage::Cache {
                self.failures.fetch_add(1, Ordering::Relaxed);
            }
        }
    }

    fn req(ino: u64) -> ThumbRequest {
        ThumbRequest {
            entry: EntryRef::new(EntryId { dev: 1, ino }, format!("/x/{ino}")),
            mime: None,
            mtime: None,
            size: ThumbSize::Normal,
        }
    }

    fn wait_ready(t: &Arc<Thumbnails>, r: &ThumbRequest) -> ThumbState {
        for _ in 0..500 {
            match t.get(r.clone(), Priority::Visible) {
                ThumbState::Pending => std::thread::sleep(std::time::Duration::from_millis(2)),
                other => return other,
            }
        }
        panic!("thumbnail never resolved");
    }

    #[test]
    fn providers_run_in_order_and_earlier_ones_store_the_result() {
        let t = Thumbnails::new(Scheduler::new(2));
        let cache = Fake::new("cache", Stage::Generate, false);
        let decoder = Fake::new("decoder", Stage::Generate, true);
        t.register("core", cache.clone());
        t.register("core", decoder.clone());
        assert!(matches!(
            t.get(req(1), Priority::Visible),
            ThumbState::Pending
        ));
        assert!(matches!(wait_ready(&t, &req(1)), ThumbState::Ready(_)));
        assert_eq!(cache.calls.load(Ordering::Relaxed), 1);
        assert_eq!(decoder.calls.load(Ordering::Relaxed), 1);
        assert_eq!(
            cache.stored.load(Ordering::Relaxed),
            1,
            "the cache provider stores what the decoder made"
        );
        assert_eq!(decoder.stored.load(Ordering::Relaxed), 0);
        // Second get is served from the LRU, no provider call.
        assert!(matches!(
            t.get(req(1), Priority::Visible),
            ThumbState::Ready(_)
        ));
        assert_eq!(decoder.calls.load(Ordering::Relaxed), 1);
    }

    /// Draws only with a turn, of which there is none until the test
    /// frees it, but file 2, which needs none.
    struct Turned {
        free: AtomicBool,
        calls: AtomicUsize,
        waiting: Arc<Mutex<Vec<Box<dyn FnOnce() + Send>>>>,
    }

    impl ThumbnailProvider for Turned {
        fn id(&self) -> &'static str {
            "turned"
        }
        fn stage(&self) -> Stage {
            Stage::LastResort
        }
        fn accepts(&self, _: &ThumbRequest) -> bool {
            true
        }
        fn thumbnail(&self, req: &ThumbRequest, _: &CancelToken) -> anyhow::Result<Option<Image>> {
            self.calls.fetch_add(1, Ordering::Relaxed);
            if req.entry.id.ino != 2 && !self.free.load(Ordering::Relaxed) {
                let waiting = Arc::clone(&self.waiting);
                return Err(Busy::new(move |then| waiting.lock().push(then)).into());
            }
            Ok(Some(Image {
                width: 2,
                height: 2,
                rgba: Arc::from(vec![0u8; 16]),
            }))
        }
    }

    #[test]
    fn a_busy_provider_holds_no_worker_and_is_asked_again_once_free() {
        // One worker takes requests: all but one of two.
        let t = Thumbnails::new(Scheduler::new(2));
        let earlier = Fake::new("earlier", Stage::Generate, false);
        let turned = Arc::new(Turned {
            free: AtomicBool::new(false),
            calls: AtomicUsize::new(0),
            waiting: Arc::default(),
        });
        t.register("core", earlier.clone());
        t.register("core", turned.clone());
        assert!(matches!(
            t.get(req(1), Priority::Visible),
            ThumbState::Pending
        ));
        for _ in 0..500 {
            if !turned.waiting.lock().is_empty() {
                break;
            }
            std::thread::sleep(Duration::from_millis(2));
        }
        assert_eq!(turned.waiting.lock().len(), 1, "set aside, waiting");
        // The worker goes on with another file meanwhile.
        assert!(matches!(wait_ready(&t, &req(2)), ThumbState::Ready(_)));
        assert!(matches!(
            t.get(req(1), Priority::Visible),
            ThumbState::Pending
        ));
        // A turn free: taken again from the provider that was busy.
        turned.free.store(true, Ordering::Relaxed);
        let waiting = std::mem::take(&mut *turned.waiting.lock());
        for then in waiting {
            then();
        }
        assert!(matches!(wait_ready(&t, &req(1)), ThumbState::Ready(_)));
        assert_eq!(
            earlier.calls.load(Ordering::Relaxed),
            2,
            "once for each file, not again for the one set aside"
        );
        assert_eq!(turned.calls.load(Ordering::Relaxed), 3);
    }

    #[test]
    fn a_failure_is_remembered_once_a_thumbnailer_tried() {
        let t = Thumbnails::new(Scheduler::new(2));
        let cache = Fake::new("cache", Stage::Cache, false);
        t.register("core", cache.clone());
        assert!(matches!(wait_ready(&t, &req(1)), ThumbState::Unavailable));
        assert_eq!(
            cache.failures.load(Ordering::Relaxed),
            0,
            "only the cache took it: nothing failed to draw it"
        );
        let maker = Fake::new("maker", Stage::Generate, false);
        t.register("core", maker.clone());
        assert!(matches!(wait_ready(&t, &req(1)), ThumbState::Unavailable));
        assert_eq!(maker.calls.load(Ordering::Relaxed), 1);
        assert_eq!(cache.failures.load(Ordering::Relaxed), 1);
        // Asked again with the failure remembered (a later launch): no
        // provider runs, nothing is written again.
        t.providers_changed();
        assert!(matches!(wait_ready(&t, &req(1)), ThumbState::Unavailable));
        assert_eq!(maker.calls.load(Ordering::Relaxed), 1);
        assert_eq!(cache.failures.load(Ordering::Relaxed), 1);
    }

    #[test]
    fn unavailable_is_remembered_and_lru_respects_its_budget() {
        let t = Thumbnails::new(Scheduler::new(2));
        let none = Fake::new("none", Stage::Generate, false);
        t.register("core", none.clone());
        assert!(matches!(wait_ready(&t, &req(7)), ThumbState::Unavailable));
        assert!(matches!(
            t.get(req(7), Priority::Visible),
            ThumbState::Unavailable
        ));
        assert_eq!(none.calls.load(Ordering::Relaxed), 1);

        let t = Thumbnails::new(Scheduler::new(2));
        t.set_budget(160); // a quarter kept here: two 16-byte images fit, three do not
        t.register("core", Fake::new("d", Stage::Generate, true));
        for i in 1..=3 {
            wait_ready(&t, &req(i));
        }
        let (_, _, bytes) = t.stats();
        assert!(bytes <= 40, "cache holds {bytes} bytes, over budget");
        assert!(
            matches!(t.get(req(1), Priority::Visible), ThumbState::Pending),
            "oldest entry was evicted"
        );
    }

    #[test]
    fn stages_order_providers_whoever_registers_first() {
        let t = Thumbnails::new(Scheduler::new(1));
        t.register("desktop", Fake::new("converter", Stage::LastResort, false));
        t.register("renderer", Fake::new("fallback", Stage::Fallback, true));
        t.register("desktop", Fake::new("thumbnailer", Stage::Generate, false));
        t.register("desktop", Fake::new("cache", Stage::Cache, false));
        t.register("other", Fake::new("decoder", Stage::Generate, false));
        assert_eq!(
            t.provider_ids(),
            ["cache", "thumbnailer", "decoder", "fallback", "converter"]
        );
        // Registered again (a settings change), the cache still comes first.
        t.unregister_owner("desktop");
        let cache = Fake::new("cache", Stage::Cache, false);
        t.register("desktop", cache.clone());
        assert_eq!(t.provider_ids()[0], "cache");
        assert!(matches!(wait_ready(&t, &req(3)), ThumbState::Ready(_)));
        assert_eq!(
            cache.stored.load(Ordering::Relaxed),
            1,
            "the cache stores what the fallback made"
        );
    }

    #[test]
    fn a_new_modification_time_is_a_new_request() {
        let t = Thumbnails::new(Scheduler::new(1));
        let maker = Fake::new("maker", Stage::Generate, false);
        t.register("core", maker.clone());
        let at = |secs| ThumbRequest {
            mtime: Some(SystemTime::UNIX_EPOCH + Duration::from_secs(secs)),
            ..req(5)
        };
        // Asked while it was still being written: nothing to show.
        assert!(matches!(wait_ready(&t, &at(10)), ThumbState::Unavailable));
        // Written: the failure is not remembered for the finished file.
        maker.produce.store(true, Ordering::Relaxed);
        let ThumbState::Ready(first) = wait_ready(&t, &at(20)) else {
            panic!("the finished file has a thumbnail");
        };
        // Edited: a new picture, not the one of the earlier version.
        let ThumbState::Ready(second) = wait_ready(&t, &at(30)) else {
            panic!("the edited file has a thumbnail");
        };
        assert!(!Arc::ptr_eq(&first, &second));
        assert_eq!(maker.calls.load(Ordering::Relaxed), 3);
    }

    #[test]
    fn providers_that_change_during_a_request_are_asked_again() {
        let t = Thumbnails::new(Scheduler::new(1));
        let finder = Fake::new("finder", Stage::Generate, false);
        t.register("core", finder.clone());
        let woken = Arc::new(AtomicUsize::new(0));
        let count = Arc::clone(&woken);
        t.set_waker(Some(Arc::new(move || {
            count.fetch_add(1, Ordering::Relaxed);
        })));
        // It finds its tools while the first request runs without them.
        let service = Arc::clone(&t);
        let found = Arc::clone(&finder);
        *finder.during.lock() = Some(Box::new(move || {
            found.produce.store(true, Ordering::Relaxed);
            service.providers_changed();
        }));
        assert!(
            matches!(wait_ready(&t, &req(9)), ThumbState::Ready(_)),
            "a miss against the old providers is not remembered"
        );
        assert_eq!(finder.calls.load(Ordering::Relaxed), 2);

        // Found after the miss was remembered: forgotten, and the UI woken.
        let late = Fake::new("late", Stage::Generate, false);
        let t = Thumbnails::new(Scheduler::new(1));
        t.register("core", late.clone());
        assert!(matches!(wait_ready(&t, &req(4)), ThumbState::Unavailable));
        late.produce.store(true, Ordering::Relaxed);
        let before = woken.load(Ordering::Relaxed);
        t.set_waker(Some(Arc::new({
            let woken = Arc::clone(&woken);
            move || {
                woken.fetch_add(1, Ordering::Relaxed);
            }
        })));
        t.providers_changed();
        assert!(woken.load(Ordering::Relaxed) > before);
        assert!(matches!(wait_ready(&t, &req(4)), ThumbState::Ready(_)));
    }

    #[test]
    fn retry_forgets_one_entry_only() {
        let t = Thumbnails::new(Scheduler::new(1));
        let maker = Fake::new("maker", Stage::Generate, false);
        t.register("core", maker.clone());
        for ino in [1, 2] {
            assert!(matches!(wait_ready(&t, &req(ino)), ThumbState::Unavailable));
        }
        maker.produce.store(true, Ordering::Relaxed);
        t.retry(EntryId { dev: 1, ino: 1 });
        assert!(matches!(wait_ready(&t, &req(1)), ThumbState::Ready(_)));
        assert!(matches!(
            t.get(req(2), Priority::Visible),
            ThumbState::Unavailable
        ));
    }
}
