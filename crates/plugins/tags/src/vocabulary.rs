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

use parking_lot::Mutex;
use skwir_core::app::App;
use skwir_core::cache::CacheSlot;
use skwir_core::cancel::CancelToken;
use skwir_core::scheduler::Priority;

/// Tags remembered at most; past this, those least recently used or seen
/// are forgotten, down to [`KEPT`].
const AT_MOST: usize = 2_000;
/// What is kept once the bound is passed: forgetting a tenth at a time,
/// not one, a library of more tags than the bound did not sort them all
/// for every new one a listing showed.
const KEPT: usize = AT_MOST - AT_MOST / 10;
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
    /// The color a file last gave it (a Mac's Finder tag's), by name.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    given: Option<String>,
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
            if known.given.is_none() {
                known.given.clone_from(&k.given);
            }
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
                    given: None,
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
            let excess = inner.known.len() - KEPT;
            // The oldest `excess` first, in no order among themselves.
            by_age.select_nth_unstable(excess - 1);
            for (_, key) in by_age.into_iter().take(excess) {
                inner.known.remove(&key);
            }
        }
        new
    }

    /// The colors files gave tags as a listing showed them (a Mac's
    /// Finder tags'), by color name. Writes only when one changed.
    pub fn given(self: &Arc<Self>, colors: &[(String, &str)]) {
        let changed = {
            let mut inner = self.inner.lock();
            let mut changed = false;
            for (tag, color) in colors {
                if let Some(known) = inner.known.get_mut(&tag.to_lowercase())
                    && known.given.as_deref() != Some(*color)
                {
                    known.given = Some((*color).to_owned());
                    changed = true;
                }
            }
            changed
        };
        if changed {
            self.write_soon();
        }
    }

    /// The color a file last gave `tag`, if one did.
    pub fn given_color(&self, tag: &str) -> Option<String> {
        self.inner
            .lock()
            .known
            .get(&tag.to_lowercase())?
            .given
            .clone()
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
    fn the_color_a_file_gave_a_tag_is_remembered_with_it() {
        let v = Arc::new(vocabulary());
        v.touch(&["Rouge".into()], false);
        v.given(&[("ROUGE".into(), "red"), ("unknown".into(), "blue")]);
        assert_eq!(v.given_color("rouge").as_deref(), Some("red"));
        assert_eq!(v.given_color("unknown"), None, "only for a tag seen");
    }

    #[test]
    fn it_forgets_the_oldest_past_its_bound() {
        let v = vocabulary();
        let tags: Vec<String> = (0..AT_MOST + 5).map(|i| format!("t{i}")).collect();
        v.touch(&tags, false);
        assert_eq!(v.inner.lock().known.len(), KEPT);
    }

    /// Past its bound it forgets a tenth at once, the oldest, and the next
    /// tags seen are taken in without forgetting any until the bound is
    /// passed again.
    #[test]
    fn it_forgets_a_tenth_at_a_time_and_keeps_the_newest() {
        let v = vocabulary();
        {
            let mut inner = v.inner.lock();
            for i in 0..AT_MOST {
                inner.known.insert(
                    format!("old{i}"),
                    Known {
                        name: format!("old{i}"),
                        used: 0,
                        seen: i as u64 + 1,
                        given: None,
                    },
                );
            }
        }
        let len = || v.inner.lock().known.len();
        v.touch(&["new0".into()], false);
        assert_eq!(len(), KEPT);
        for i in 1..=AT_MOST - KEPT {
            v.touch(&[format!("new{i}")], false);
        }
        assert_eq!(len(), AT_MOST, "none forgotten below the bound");
        v.touch(&["new-last".into()], false);
        assert_eq!(len(), KEPT);
        let inner = v.inner.lock();
        assert!(inner.known.contains_key("new-last"));
        assert!(
            (0..=AT_MOST - KEPT).all(|i| inner.known.contains_key(&format!("new{i}"))),
            "the tags seen now outlive the old ones"
        );
        assert!(!inner.known.contains_key("old0"), "the oldest forgotten");
    }
}
