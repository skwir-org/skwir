//! The thumbnail service.
//!
//! Providers (registered by the platform crate and plugins) produce decoded
//! RGBA images for an entry: a cache reader, an image decoder, external
//! thumbnailers, and so on. The service tries them in order on a worker,
//! most-recently-requested first, drops requests the viewport scrolled away
//! from, keeps a byte-budgeted LRU of results, and wakes the UI when one
//! lands. Toolkit-specific GPU upload is the UI's job.

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

/// A thumbnail source. Tried in registration order.
pub trait ThumbnailProvider: Send + Sync {
    fn id(&self) -> &'static str;
    /// Cheap pre-check so the queue is not filled with hopeless work.
    fn accepts(&self, req: &ThumbRequest) -> bool;
    /// Find or make the thumbnail. `Ok(None)` means "not from me, try the next".
    fn thumbnail(&self, req: &ThumbRequest, cancel: &CancelToken) -> anyhow::Result<Option<Image>>;
    /// Persist a thumbnail another provider produced (cache writers).
    fn store(&self, _req: &ThumbRequest, _image: &Image) {}
}

/// What the UI gets back synchronously.
#[derive(Clone, Debug)]
pub enum ThumbState {
    Ready(Arc<Image>),
    /// Queued or in progress; the waker fires when it lands.
    Pending,
    /// No provider could produce one.
    Unavailable,
}

type Key = (EntryId, ThumbSize);

struct Queued {
    key: Key,
    req: ThumbRequest,
    priority: Priority,
    /// The frame it was last asked for: older than a few frames, the
    /// viewport has moved on and it is dropped before it runs.
    frame: u64,
    seq: u64,
}

struct State {
    lru: ByteLru<Key, Arc<Image>>,
    unavailable: HashSet<Key>,
    /// Pending requests; workers take the best priority, most recent first.
    queue: Vec<Queued>,
    /// Running requests: the frame they were last asked for, and the token
    /// that stops their provider once nobody asks any more.
    in_flight: HashMap<Key, (u64, CancelToken)>,
    frame: u64,
    seq: u64,
    decoded: u64,
    skipped: u64,
}

/// Frames a request may go unasked before it is dropped or cancelled.
const STALE_FRAMES: u64 = 3;

/// The service. One per app, shared with workers.
pub struct Thumbnails {
    providers: RwLock<Vec<(Arc<str>, Arc<dyn ThumbnailProvider>)>>,
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
                lru: ByteLru::new(64 << 20),
                unavailable: HashSet::new(),
                queue: Vec::new(),
                in_flight: HashMap::new(),
                frame: 0,
                seq: 0,
                decoded: 0,
                skipped: 0,
            }),
            scheduler,
            waker: Mutex::new(None),
            lifetime,
        })
    }

    pub fn set_waker(&self, waker: Option<Waker>) {
        *self.waker.lock() = waker;
    }

    /// Bytes of decoded images to keep. Default 64 MiB.
    pub fn set_budget(&self, bytes: usize) {
        let evicted = self.state.lock().lru.set_budget(bytes);
        drop(evicted);
    }

    pub fn register(&self, owner: &str, provider: Arc<dyn ThumbnailProvider>) {
        self.providers.write().push((owner.into(), provider));
        self.state.lock().unavailable.clear();
    }

    pub fn unregister_owner(&self, owner: &str) {
        self.providers.write().retain(|(o, _)| &**o != owner);
    }

    pub fn provider_ids(&self) -> Vec<&'static str> {
        self.providers.read().iter().map(|(_, p)| p.id()).collect()
    }

    /// Call once per rendered frame: requests not asked for in the last few
    /// frames are dropped before they run, and running ones are cancelled,
    /// since the viewport moved on.
    pub fn tick(&self) {
        let mut s = self.state.lock();
        s.frame += 1;
        let frame = s.frame;
        for (asked, cancel) in s.in_flight.values() {
            if frame.saturating_sub(*asked) > STALE_FRAMES {
                cancel.cancel();
            }
        }
    }

    /// Ask for a thumbnail. Returns immediately.
    pub fn get(self: &Arc<Self>, req: ThumbRequest, priority: Priority) -> ThumbState {
        let key = (req.entry.id, req.size);
        {
            let mut s = self.state.lock();
            if let Some(img) = s.lru.get(&key) {
                return ThumbState::Ready(Arc::clone(img));
            }
            if s.unavailable.contains(&key) {
                return ThumbState::Unavailable;
            }
            let frame = s.frame;
            if let Some((asked, _)) = s.in_flight.get_mut(&key) {
                *asked = frame;
                if let Some(q) = s.queue.iter_mut().find(|q| q.key == key) {
                    q.frame = frame;
                    q.priority = q.priority.min(priority);
                }
                return ThumbState::Pending;
            }
        }
        // Plugin code runs outside the state lock.
        let accepted = {
            let providers = self.providers.read();
            providers.iter().any(|(_, p)| p.accepts(&req))
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
        s.in_flight.insert(key, (frame, self.lifetime.child()));
        s.seq += 1;
        let seq = s.seq;
        s.queue.push(Queued {
            key,
            req,
            priority,
            frame,
            seq,
        });
        drop(s);
        self.spawn_worker(priority);
        ThumbState::Pending
    }

    /// Forget everything about an entry (it changed or was removed).
    pub fn invalidate(&self, id: EntryId) {
        let mut s = self.state.lock();
        for size in [ThumbSize::Normal, ThumbSize::Large] {
            s.lru.remove(&(id, size));
            s.unavailable.remove(&(id, size));
        }
    }

    /// (decoded, skipped as stale, cached bytes)
    pub fn stats(&self) -> (u64, u64, usize) {
        let s = self.state.lock();
        (s.decoded, s.skipped, s.lru.bytes())
    }

    fn spawn_worker(self: &Arc<Self>, priority: Priority) {
        let this = Arc::clone(self);
        self.scheduler
            .spawn(priority, "thumb", self.lifetime.child(), move || {
                this.work_one();
            });
    }

    /// Take the best live request (visible first, most recent first) and run
    /// the providers on it.
    fn work_one(self: &Arc<Self>) {
        let (key, req, cancel) = {
            let mut s = self.state.lock();
            loop {
                let best = s
                    .queue
                    .iter()
                    .enumerate()
                    .min_by_key(|(_, q)| (q.priority, std::cmp::Reverse(q.seq)))
                    .map(|(i, _)| i);
                let Some(i) = best else {
                    return;
                };
                let q = s.queue.swap_remove(i);
                let cancel = s.in_flight.get(&q.key).map(|(_, c)| c.clone());
                match cancel {
                    Some(cancel)
                        if s.frame.saturating_sub(q.frame) <= STALE_FRAMES
                            && !cancel.is_cancelled() =>
                    {
                        break (q.key, q.req, cancel);
                    }
                    _ => {
                        s.in_flight.remove(&q.key);
                        s.skipped += 1;
                    }
                }
            }
        };
        let providers: Vec<Arc<dyn ThumbnailProvider>> = self
            .providers
            .read()
            .iter()
            .map(|(_, p)| Arc::clone(p))
            .collect();
        let mut produced: Option<(usize, Image)> = None;
        for (i, p) in providers.iter().enumerate() {
            if cancel.is_cancelled() {
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
                Err(e) => {
                    tracing::debug!(provider = p.id(), path = %req.entry.path.display(), "thumbnail failed: {e:#}");
                }
            }
        }
        let cancelled = cancel.is_cancelled();
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
                None => {
                    s.unavailable.insert(key);
                    Vec::new()
                }
            }
        };
        drop(evicted);
        if let Some(w) = self.waker.lock().as_ref() {
            w();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    struct Fake {
        id: &'static str,
        calls: AtomicUsize,
        produce: bool,
        stored: AtomicUsize,
    }

    impl ThumbnailProvider for Fake {
        fn id(&self) -> &'static str {
            self.id
        }
        fn accepts(&self, _: &ThumbRequest) -> bool {
            true
        }
        fn thumbnail(&self, _: &ThumbRequest, _: &CancelToken) -> anyhow::Result<Option<Image>> {
            self.calls.fetch_add(1, Ordering::Relaxed);
            Ok(self.produce.then(|| Image {
                width: 2,
                height: 2,
                rgba: Arc::from(vec![0u8; 16]),
            }))
        }
        fn store(&self, _: &ThumbRequest, _: &Image) {
            self.stored.fetch_add(1, Ordering::Relaxed);
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
        let cache = Arc::new(Fake {
            id: "cache",
            calls: AtomicUsize::new(0),
            produce: false,
            stored: AtomicUsize::new(0),
        });
        let decoder = Arc::new(Fake {
            id: "decoder",
            calls: AtomicUsize::new(0),
            produce: true,
            stored: AtomicUsize::new(0),
        });
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

    #[test]
    fn unavailable_is_remembered_and_lru_respects_its_budget() {
        let t = Thumbnails::new(Scheduler::new(2));
        let none = Arc::new(Fake {
            id: "none",
            calls: AtomicUsize::new(0),
            produce: false,
            stored: AtomicUsize::new(0),
        });
        t.register("core", none.clone());
        assert!(matches!(wait_ready(&t, &req(7)), ThumbState::Unavailable));
        assert!(matches!(
            t.get(req(7), Priority::Visible),
            ThumbState::Unavailable
        ));
        assert_eq!(none.calls.load(Ordering::Relaxed), 1);

        let t = Thumbnails::new(Scheduler::new(2));
        t.set_budget(40); // two 16-byte images fit, three do not
        t.register(
            "core",
            Arc::new(Fake {
                id: "d",
                calls: AtomicUsize::new(0),
                produce: true,
                stored: AtomicUsize::new(0),
            }),
        );
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
}
