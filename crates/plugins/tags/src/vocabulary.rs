//! The tags the user has: those seen on files as folders were listed, and
//! those given, with when each was last used. It only suggests (the tag
//! editor offers the tags used last first), so it lives in the deletable
//! cache: deleting it forgets suggestions, never a tag, which stays on the
//! files. It is bounded, and writes only when a tag is used or a new one is
//! seen, on a worker.

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Weak};
use std::time::{SystemTime, UNIX_EPOCH};

use bibli_core::app::App;
use bibli_core::cache::CacheSlot;
use bibli_core::cancel::CancelToken;
use bibli_core::scheduler::Priority;
use parking_lot::Mutex;

/// Tags remembered at most; past this, the one least recently used or seen
/// is forgotten.
const AT_MOST: usize = 2_000;
const KEY: &[u8] = b"vocabulary";

#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
struct Known {
    /// As first spelled.
    name: String,
    /// When the user last gave it, in seconds; 0 if only ever seen.
    #[serde(default)]
    used: u64,
    /// When a listing last showed it on a file, in seconds.
    #[serde(default)]
    seen: u64,
}

impl Known {
    fn last(&self) -> u64 {
        self.used.max(self.seen)
    }
}

#[derive(Default)]
struct Inner {
    /// By the tag's lowercase form: tags ignore case.
    known: HashMap<String, Known>,
    loaded: bool,
}

pub struct Vocabulary {
    inner: Mutex<Inner>,
    slot: Option<CacheSlot>,
    app: Weak<App>,
    lifetime: CancelToken,
    /// A write is on its way: later changes ride along with it.
    writing: AtomicBool,
}

fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_secs())
}

impl Vocabulary {
    /// Reads what the cache remembers on a worker; tags seen or used before
    /// it answers are kept.
    pub fn new(slot: Option<CacheSlot>, app: &App, lifetime: CancelToken) -> Arc<Self> {
        let vocabulary = Arc::new(Vocabulary {
            inner: Mutex::default(),
            slot,
            app: app.weak(),
            lifetime,
            writing: AtomicBool::new(false),
        });
        let loading = Arc::clone(&vocabulary);
        app.scheduler.spawn(
            Priority::Background,
            "tags.vocabulary",
            vocabulary.lifetime.child(),
            move || loading.load(),
        );
        vocabulary
    }

    fn load(&self) {
        let stored: Vec<Known> = self
            .slot
            .as_ref()
            .and_then(|slot| slot.get(KEY))
            .and_then(|bytes| serde_json::from_slice(&bytes).ok())
            .unwrap_or_default();
        let mut inner = self.inner.lock();
        for k in stored {
            let entry = inner.known.entry(k.name.to_lowercase());
            let known = entry.or_insert_with(|| k.clone());
            known.used = known.used.max(k.used);
            known.seen = known.seen.max(k.seen);
        }
        inner.loaded = true;
    }

    /// Tags a listing showed. Writes only when one is new.
    pub fn saw(self: &Arc<Self>, tags: &[String]) {
        if self.touch(tags, false) {
            self.write_soon();
        }
    }

    /// Tags the user gave.
    pub fn used(self: &Arc<Self>, tags: &[String]) {
        if !tags.is_empty() {
            self.touch(tags, true);
            self.write_soon();
        }
    }

    /// Record `tags` now; whether one of them was new.
    fn touch(&self, tags: &[String], used: bool) -> bool {
        let at = now();
        let mut inner = self.inner.lock();
        let mut new = false;
        for tag in tags {
            let known = inner.known.entry(tag.to_lowercase()).or_insert_with(|| {
                new = true;
                Known {
                    name: tag.clone(),
                    used: 0,
                    seen: 0,
                }
            });
            if used {
                known.used = at;
            } else {
                known.seen = at;
            }
        }
        if inner.known.len() > AT_MOST {
            let mut by_age: Vec<(u64, String)> = inner
                .known
                .iter()
                .map(|(key, k)| (k.last(), key.clone()))
                .collect();
            by_age.sort_unstable();
            let excess = inner.known.len() - AT_MOST;
            for (_, key) in by_age.into_iter().take(excess) {
                inner.known.remove(&key);
            }
        }
        new
    }

    /// The tags known, the most recently used first, then those seen.
    pub fn known(&self) -> Vec<String> {
        let inner = self.inner.lock();
        let mut known: Vec<&Known> = inner.known.values().collect();
        known.sort_by(|a, b| {
            b.used
                .cmp(&a.used)
                .then(b.seen.cmp(&a.seen))
                .then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase()))
        });
        known.into_iter().map(|k| k.name.clone()).collect()
    }

    fn write_soon(self: &Arc<Self>) {
        let Some(app) = self.app.upgrade().filter(|_| self.slot.is_some()) else {
            return;
        };
        if self.writing.swap(true, Ordering::AcqRel) {
            return;
        }
        let this = Arc::clone(self);
        app.scheduler.spawn(
            Priority::Background,
            "tags.vocabulary",
            self.lifetime.child(),
            move || {
                this.writing.store(false, Ordering::Release);
                this.write();
            },
        );
    }

    fn write(&self) {
        let Some(slot) = &self.slot else { return };
        let bytes = {
            let inner = self.inner.lock();
            // Before the cache answered, what it holds would be lost.
            if !inner.loaded {
                return;
            }
            let known: Vec<&Known> = inner.known.values().collect();
            serde_json::to_vec(&known)
        };
        if let Ok(bytes) = bytes
            && let Err(error) = slot.put(KEY, &bytes)
        {
            tracing::debug!(%error, "tags: the vocabulary was not written");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn vocabulary() -> Vocabulary {
        Vocabulary {
            inner: Mutex::new(Inner {
                known: HashMap::new(),
                loaded: true,
            }),
            slot: None,
            app: Weak::new(),
            lifetime: CancelToken::new(),
            writing: AtomicBool::new(false),
        }
    }

    #[test]
    fn used_tags_come_first_and_case_does_not_make_a_new_one() {
        let v = vocabulary();
        assert!(v.touch(&["Work".into(), "home".into()], false));
        assert!(!v.touch(&["work".into()], false), "the same tag");
        v.inner.lock().known.get_mut("home").unwrap().used = 5;
        assert_eq!(v.known(), ["home", "Work"]);
        v.inner.lock().known.get_mut("work").unwrap().used = 9;
        assert_eq!(v.known(), ["Work", "home"], "first spelling kept");
    }

    #[test]
    fn it_forgets_the_oldest_past_its_bound() {
        let v = vocabulary();
        let tags: Vec<String> = (0..AT_MOST + 5).map(|i| format!("t{i}")).collect();
        v.touch(&tags, false);
        assert_eq!(v.inner.lock().known.len(), AT_MOST);
    }
}
