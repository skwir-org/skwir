//! The thumbnail service.
//!
//! Providers (registered by the platform crate and plugins) produce decoded
//! RGBA images for an entry: a cache reader, an image decoder, external
//! thumbnailers, and so on. The service tries them in order on a worker,
//! most-recently-requested first, drops requests the viewport scrolled away
//! from, keeps a byte-budgeted LRU of results, and wakes the UI when one
//! lands. Toolkit-specific GPU upload is the UI's job.

use std::collections::{HashMap, HashSet, VecDeque};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::SystemTime;

use parking_lot::{Mutex, RwLock};
use serde::{Deserialize, Serialize};

use crate::cancel::CancelToken;
use crate::entry::EntryId;
use crate::location::Waker;
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

struct Lru {
    map: HashMap<Key, (Arc<Image>, u64)>,
    order: VecDeque<(Key, u64)>,
    bytes: usize,
    budget: usize,
    tick: u64,
}

impl Lru {
    fn get(&mut self, k: &Key) -> Option<Arc<Image>> {
        self.tick += 1;
        let tick = self.tick;
        let (img, stamp) = self.map.get_mut(k)?;
        *stamp = tick;
        self.order.push_back((*k, tick));
        Some(Arc::clone(img))
    }

    fn insert(&mut self, k: Key, img: &Arc<Image>) {
        self.tick += 1;
        if let Some((old, _)) = self.map.insert(k, (Arc::clone(img), self.tick)) {
            self.bytes -= old.bytes();
        }
        self.bytes += img.bytes();
        self.order.push_back((k, self.tick));
        self.evict();
    }

    fn evict(&mut self) {
        while self.bytes > self.budget {
            let Some((old_k, stamp)) = self.order.pop_front() else {
                break;
            };
            if self.map.get(&old_k).is_some_and(|(_, s)| *s == stamp) {
                let (old, _) = self.map.remove(&old_k).expect("present");
                self.bytes -= old.bytes();
            }
        }
    }

    fn remove(&mut self, k: &Key) {
        if let Some((old, _)) = self.map.remove(k) {
            self.bytes -= old.bytes();
        }
    }
}

struct State {
    lru: Lru,
    unavailable: HashSet<Key>,
    /// Pending requests, most recent last; workers pop from the back.
    queue: Vec<(Key, ThumbRequest, Priority, u64)>,
    in_flight: HashSet<Key>,
    frame: u64,
    decoded: u64,
    skipped: u64,
}

/// The service. One per app, shared with workers.
pub struct Thumbnails {
    providers: RwLock<Vec<(Arc<str>, Arc<dyn ThumbnailProvider>)>>,
    state: Mutex<State>,
    scheduler: Scheduler,
    waker: Mutex<Option<Waker>>,
    seq: AtomicU64,
}

impl Thumbnails {
    pub fn new(scheduler: Scheduler) -> Arc<Self> {
        Arc::new(Thumbnails {
            providers: RwLock::new(Vec::new()),
            state: Mutex::new(State {
                lru: Lru {
                    map: HashMap::new(),
                    order: VecDeque::new(),
                    bytes: 0,
                    budget: 64 << 20,
                    tick: 0,
                },
                unavailable: HashSet::new(),
                queue: Vec::new(),
                in_flight: HashSet::new(),
                frame: 0,
                decoded: 0,
                skipped: 0,
            }),
            scheduler,
            waker: Mutex::new(None),
            seq: AtomicU64::new(0),
        })
    }

    pub fn set_waker(&self, waker: Option<Waker>) {
        *self.waker.lock() = waker;
    }

    /// Bytes of decoded images to keep. Default 64 MiB.
    pub fn set_budget(&self, bytes: usize) {
        let mut s = self.state.lock();
        s.lru.budget = bytes;
        s.lru.evict();
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

    /// Call once per rendered frame: requests older than a few frames are
    /// dropped before they run, since the viewport moved on.
    pub fn tick(&self) {
        self.state.lock().frame += 1;
    }

    /// Ask for a thumbnail. Returns immediately.
    pub fn get(self: &Arc<Self>, req: ThumbRequest, priority: Priority) -> ThumbState {
        let key = (req.entry.id, req.size);
        let mut s = self.state.lock();
        if let Some(img) = s.lru.get(&key) {
            return ThumbState::Ready(img);
        }
        if s.unavailable.contains(&key) {
            return ThumbState::Unavailable;
        }
        if !s.in_flight.contains(&key) {
            let accepted = self.providers.read().iter().any(|(_, p)| p.accepts(&req));
            if !accepted {
                s.unavailable.insert(key);
                return ThumbState::Unavailable;
            }
            s.in_flight.insert(key);
            let frame = s.frame;
            s.queue.push((key, req, priority, frame));
            drop(s);
            self.spawn_worker(priority);
        }
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
        (s.decoded, s.skipped, s.lru.bytes)
    }

    fn spawn_worker(self: &Arc<Self>, priority: Priority) {
        let this = Arc::clone(self);
        let seq = self.seq.fetch_add(1, Ordering::Relaxed);
        let _ = seq;
        self.scheduler
            .spawn(priority, "thumb", CancelToken::new(), move || {
                this.work_one();
            });
    }

    /// Pop the most recent live request and run the providers on it.
    fn work_one(self: &Arc<Self>) {
        let (key, req) = {
            let mut s = self.state.lock();
            loop {
                let Some((key, req, _, frame)) = s.queue.pop() else {
                    return;
                };
                if s.frame.saturating_sub(frame) > 3 {
                    s.in_flight.remove(&key);
                    s.skipped += 1;
                    continue;
                }
                break (key, req);
            }
        };
        let providers: Vec<Arc<dyn ThumbnailProvider>> = self
            .providers
            .read()
            .iter()
            .map(|(_, p)| Arc::clone(p))
            .collect();
        let cancel = CancelToken::new();
        let mut produced: Option<(usize, Image)> = None;
        for (i, p) in providers.iter().enumerate() {
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
        if let Some((i, img)) = &produced {
            for p in &providers[..*i] {
                p.store(&req, img);
            }
        }
        {
            let mut s = self.state.lock();
            s.in_flight.remove(&key);
            match produced {
                Some((_, img)) => {
                    s.decoded += 1;
                    s.lru.insert(key, &Arc::new(img));
                }
                None => {
                    s.unavailable.insert(key);
                }
            }
        }
        if let Some(w) = self.waker.lock().as_ref() {
            w();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::AtomicUsize;

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
