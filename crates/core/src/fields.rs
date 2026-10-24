//! Fields filled on demand (ADR-023): what a file says that is too costly
//! to read for every listing, read only when something asks. A picture's
//! position is the first: reading EXIF during hydration would slow every
//! folder for users who never search by place (ADR-020), but a search
//! with `near:` and Properties want it.
//!
//! A plugin registers a [`FieldProvider`] for the keys it answers; a
//! caller on a worker asks [`Fields::get`] for a key of a file, or
//! [`Fields::get_all`] for several, which a provider answers from one read
//! of the file. The answer, a value or nothing, is kept by the file's
//! identity (device, inode, size, modification time to the nanosecond) in
//! memory and in the deletable cache, bounded (`KEPT_BYTES`), so a file is
//! read once until it changes; one whose identity the caller knows (a
//! listing's) is not stat-ed. Nothing here runs on the UI thread or per
//! frame.
use std::collections::HashMap;
use std::ffi::OsStr;
use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use parking_lot::{Mutex, RwLock};

use crate::cache::CacheSlot;
use crate::cancel::CancelToken;
use crate::entry::Value;

/// Answers remembered in memory; beyond, the cache on disk answers.
const REMEMBERED: usize = 100_000;
/// What the answers kept on disk may take.
const KEPT_BYTES: u64 = 16 << 20;
/// Earlier entries could encode failed reads as null. A new format key
/// prevents those legacy failures from remaining permanent negative answers.
const CACHE_FORMAT: u8 = 2;

/// Reads one or more fields of files, when asked.
pub trait FieldProvider: Send + Sync {
    fn id(&self) -> &'static str;
    /// Advanced when what it answers changes: answers kept from another
    /// version are read again.
    fn version(&self) -> u32 {
        1
    }
    /// The keys it answers: core keys (`entry::keys::POSITION`) or its
    /// own, namespaced (`geo.place`).
    fn keys(&self) -> &'static [&'static str];
    /// The words Properties labels `key` with; `None` keeps it out of
    /// Properties (a position, which the picture's own facts show).
    fn label(&self, _key: &str) -> Option<&'static str> {
        None
    }
    /// Whom Properties credits for `key`'s answers, when their data asks
    /// for it (a licence's attribution). Asked on a worker.
    fn credit(&self, _key: &str) -> Option<String> {
        None
    }
    /// Cheap, from the name alone: whether a file of this name may have
    /// `key`. Asked before anything is read.
    fn accepts(&self, key: &str, name: &OsStr) -> bool;
    /// What the file at `path` says of `key`, read now, on a worker.
    /// `Ok(None)` when it says nothing. An error is retryable, never kept
    /// as evidence that the file has no value (a disabled helper may fail).
    fn read(&self, key: &str, path: &Path, cancel: &CancelToken) -> anyhow::Result<Option<Value>>;
    /// What the file says of each of `keys`, in their order: a provider
    /// whose keys come from one read of the file (a picture's position and
    /// its clock) reads it once. Each key apart, by default.
    fn read_keys(
        &self,
        keys: &[&str],
        path: &Path,
        cancel: &CancelToken,
    ) -> Vec<anyhow::Result<Option<Value>>> {
        keys.iter()
            .map(|key| self.read(key, path, cancel))
            .collect()
    }
}

/// A file's identity: its answers hold while it is the same. Its file, its
/// size, and when it was modified, in nanoseconds since 1970. Kept on disk
/// by what its file is kept as (`FileId::kept_as`): a file whose number a
/// mount made up, by its path.
pub type Identity = (crate::entry::FileId, u64, u64);

/// Registry generation, provider id and cache version, captured before a read.
type ProviderStamp = (u64, &'static str, u32);
type MemoryKey = (ProviderStamp, Arc<str>, Identity);

#[derive(Default)]
pub struct Fields {
    providers: RwLock<Vec<(Arc<str>, Arc<dyn FieldProvider>)>>,
    /// `None` keeps answers in memory only (tests, tools).
    cache: Option<CacheSlot>,
    /// The registry generation belongs to the answer too. An old read
    /// must never fill the cache of a replacement provider for this key.
    remembered: Mutex<HashMap<MemoryKey, Option<Value>>>,
    revision: AtomicU64,
}

impl Fields {
    pub fn new(cache: CacheSlot) -> Self {
        Fields {
            cache: Some(cache),
            ..Fields::default()
        }
    }

    pub fn register(&self, owner: &str, provider: Arc<dyn FieldProvider>) {
        let mut providers = self.providers.write();
        providers.push((owner.into(), provider));
        self.changed();
    }

    pub fn unregister_owner(&self, owner: &str) {
        let mut providers = self.providers.write();
        let before = providers.len();
        providers.retain(|(o, _)| &**o != owner);
        if providers.len() != before {
            self.changed();
        }
    }

    /// Advanced when providers come or go: what was asked may be answered
    /// otherwise now.
    pub fn revision(&self) -> u64 {
        self.revision.load(Ordering::Acquire)
    }

    fn changed(&self) {
        // The caller holds providers for writing. Keep the memory lock
        // through the revision change, paired with remember's check.
        let mut remembered = self.remembered.lock();
        self.revision.fetch_add(1, Ordering::AcqRel);
        remembered.clear();
    }

    /// The ids of the providers, for a plugin's toggle to be checked.
    pub fn ids(&self) -> Vec<&'static str> {
        self.providers.read().iter().map(|(_, p)| p.id()).collect()
    }

    /// Whether some provider answers `key`.
    pub fn knows(&self, key: &str) -> bool {
        self.providers
            .read()
            .iter()
            .any(|(_, p)| p.keys().contains(&key))
    }

    /// The keys shown in Properties for a file named `name`, with their
    /// labels, in the order they were registered.
    pub fn shown_for(&self, name: &OsStr) -> Vec<(&'static str, &'static str)> {
        let mut shown = Vec::new();
        for (_, provider) in self.providers.read().iter() {
            for key in provider.keys() {
                if let Some(label) = provider.label(key)
                    && provider.accepts(key, name)
                    && !shown.iter().any(|(k, _)| k == key)
                {
                    shown.push((*key, label));
                }
            }
        }
        shown
    }

    /// Whom Properties credits for `key`, for a file named `name`. On a worker.
    pub fn credit(&self, key: &str, name: &OsStr) -> Option<String> {
        self.provider(key, name)?.1.credit(key)
    }

    /// The keys some provider may answer for a file named `name`, in the
    /// order they were registered.
    pub fn keys_for(&self, name: &OsStr) -> Vec<&'static str> {
        let mut keys = Vec::new();
        for (_, provider) in self.providers.read().iter() {
            for key in provider.keys() {
                if !keys.contains(key) && provider.accepts(key, name) {
                    keys.push(*key);
                }
            }
        }
        keys
    }

    fn provider(&self, key: &str, name: &OsStr) -> Option<(u64, Arc<dyn FieldProvider>)> {
        // Snapshot the provider and its generation under the same lock.
        let providers = self.providers.read();
        providers
            .iter()
            .find(|(_, p)| p.keys().contains(&key) && p.accepts(key, name))
            .map(|(_, p)| (self.revision(), Arc::clone(p)))
    }

    /// The value of `key` for the file at `path`: remembered, kept in the
    /// cache, or read now. On a worker, in `path`'s lane
    /// (`fs::reach::lane`, ADR-077): it may stat and read the file, which
    /// waits for good on a server that stopped answering.
    /// `None` when no provider answers it, the file says nothing, or it
    /// cannot be read.
    pub fn get(&self, key: &str, path: &Path, cancel: &CancelToken) -> Option<Value> {
        self.get_all(&[key], path, None, cancel).pop().flatten()
    }

    /// The values of `keys` for the file at `path`, in their order, as
    /// [`Fields::get`] says each, on a worker in `path`'s lane: those one
    /// provider answers are read together ([`FieldProvider::read_keys`]),
    /// on the caller's thread. With `known`, the caller
    /// says the file's identity, and that `path` is a file and no link (a
    /// listing's entry, stat-ed already): the file is not stat-ed again.
    pub fn get_all(
        &self,
        keys: &[&str],
        path: &Path,
        known: Option<Identity>,
        cancel: &CancelToken,
    ) -> Vec<Option<Value>> {
        let mut answers = vec![None; keys.len()];
        // A link is asked as what it leads to, which is what is read.
        let (content, identity) = if let Some(identity) = known {
            (std::borrow::Cow::Borrowed(path), identity)
        } else {
            let (Some(content), Some(identity)) =
                (crate::fs::content_path(path), identity_of(path))
            else {
                return answers;
            };
            (content, identity)
        };
        let Some(name) = content.file_name() else {
            return answers;
        };
        // Each key remembered, kept, or to read, with the others its
        // provider answers.
        let mut asked: Vec<(ProviderStamp, Arc<dyn FieldProvider>, Vec<usize>)> = Vec::new();
        for (i, key) in keys.iter().enumerate() {
            let Some((generation, provider)) = self.provider(key, name) else {
                continue;
            };
            let stamp = (generation, provider.id(), provider.version());
            let memory_key = (stamp, (*key).into(), identity);
            if let Some(value) = self.remembered.lock().get(&memory_key) {
                answers[i].clone_from(value);
                continue;
            }
            let kept = self
                .cache
                .as_ref()
                .and_then(|cache| cache.get(&disk_key(key, (stamp.1, stamp.2), identity, &content)))
                .and_then(|bytes| serde_json::from_slice::<Option<Value>>(&bytes).ok());
            if let Some(value) = kept {
                self.remember(memory_key, value.clone());
                answers[i] = value;
                continue;
            }
            match asked
                .iter_mut()
                .find(|(g, p, _)| *g == stamp && Arc::ptr_eq(p, &provider))
            {
                Some((_, _, at)) => at.push(i),
                None => asked.push((stamp, provider, vec![i])),
            }
        }
        for (stamp, provider, at) in asked {
            let read: Vec<&str> = at.iter().map(|&i| keys[i]).collect();
            let values = provider.read_keys(&read, path, cancel);
            // A read cut short by a cancel says nothing of the file.
            if cancel.is_cancelled() {
                return answers;
            }
            for (&i, value) in at.iter().zip(values) {
                let key = keys[i];
                let value = match value {
                    Ok(value) => value,
                    Err(error) => {
                        tracing::debug!(%error, key, "field not read");
                        continue;
                    }
                };
                // A removed provider's completion is only for its original
                // caller. Never start persisting it after its withdrawal.
                if stamp.0 == self.revision()
                    && let Some(cache) = &self.cache
                    && let Ok(bytes) = serde_json::to_vec(&value)
                {
                    match cache.put(
                        &disk_key(key, (stamp.1, stamp.2), identity, &content),
                        &bytes,
                    ) {
                        Ok(()) => cache.trim_soon(bytes.len() as u64, KEPT_BYTES),
                        Err(error) => tracing::debug!(%error, "field not kept"),
                    }
                }
                self.remember((stamp, key.into(), identity), value.clone());
                answers[i] = value;
            }
        }
        answers
    }

    fn remember(&self, key: MemoryKey, value: Option<Value>) {
        let mut remembered = self.remembered.lock();
        if key.0.0 != self.revision() {
            return;
        }
        if remembered.len() >= REMEMBERED {
            remembered.clear();
        }
        remembered.insert(key, value);
    }
}

/// The identity of the file at `path`, through a link: `None` for what is
/// no file, or cannot be stat-ed.
fn identity_of(path: &Path) -> Option<Identity> {
    let meta = std::fs::metadata(path).ok()?;
    if !meta.is_file() {
        return None;
    }
    let modified = meta
        .modified()
        .ok()
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map_or(0, |d| u64::try_from(d.as_nanos()).unwrap_or(u64::MAX));
    let id = crate::fs::stat::identity(path, &meta)?;
    Some((id, meta.len(), modified))
}

/// Where the answer of `provider` for `key` of the file at `path` is kept
/// on disk.
fn disk_key(key: &str, provider: (&str, u32), identity: Identity, path: &Path) -> Vec<u8> {
    let (file, len, modified) = identity;
    [
        format!("{CACHE_FORMAT}:{key}:{}:{}:", provider.0, provider.1).as_bytes(),
        &file.kept_as(path),
        format!(":{len}:{modified}").as_bytes(),
    ]
    .concat()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::AtomicUsize;

    struct Answer {
        id: &'static str,
        version: u32,
        value: u64,
        reads: Arc<AtomicUsize>,
        name: Option<&'static str>,
        fail: bool,
        wait: Option<(
            crossbeam_channel::Sender<()>,
            crossbeam_channel::Receiver<()>,
        )>,
    }

    impl FieldProvider for Answer {
        fn id(&self) -> &'static str {
            self.id
        }
        fn version(&self) -> u32 {
            self.version
        }
        fn keys(&self) -> &'static [&'static str] {
            &["test.answer"]
        }
        fn accepts(&self, _: &str, name: &OsStr) -> bool {
            self.name.is_none_or(|accepted| name == accepted)
        }
        fn read(&self, _: &str, _: &Path, _: &CancelToken) -> anyhow::Result<Option<Value>> {
            self.reads.fetch_add(1, Ordering::SeqCst);
            if let Some((started, resume)) = &self.wait {
                started.send(()).unwrap();
                let _ = resume.recv_timeout(std::time::Duration::from_secs(10));
            }
            anyhow::ensure!(!self.fail, "the provider stopped");
            Ok(Some(Value::UInt(self.value)))
        }
    }

    #[test]
    fn a_replaced_providers_late_read_cannot_fill_the_current_memory_cache() {
        for (id, version, disk) in [
            ("replacement", 1, true),
            ("original", 2, true),
            ("original", 1, false),
        ] {
            for warm in [false, true] {
                let tmp = tempfile::tempdir().unwrap();
                let note = tmp.path().join("it's an answer.txt");
                std::fs::write(&note, "fixture").unwrap();
                let fields = if disk {
                    Fields::new(
                        crate::cache::Cache::new(tmp.path().join("cache")).namespace("fields"),
                    )
                } else {
                    Fields::default()
                };
                let (started, entered) = crossbeam_channel::bounded(1);
                let (resume, waiting) = crossbeam_channel::bounded(0);
                fields.register(
                    "old",
                    Arc::new(Answer {
                        id: "original",
                        version: 1,
                        value: 1,
                        reads: Arc::default(),
                        name: None,
                        fail: false,
                        wait: Some((started, waiting)),
                    }),
                );
                std::thread::scope(|scope| {
                    let reading =
                        scope.spawn(|| fields.get("test.answer", &note, &CancelToken::new()));
                    // Drop resume on any assertion failure, so the old reader
                    // cannot keep the scoped thread waiting indefinitely.
                    let release = resume;
                    entered
                        .recv_timeout(std::time::Duration::from_secs(10))
                        .unwrap();
                    fields.unregister_owner("old");
                    let reads = Arc::new(AtomicUsize::new(0));
                    fields.register(
                        "new",
                        Arc::new(Answer {
                            id,
                            version,
                            value: 2,
                            reads: reads.clone(),
                            name: None,
                            fail: false,
                            wait: None,
                        }),
                    );
                    if warm {
                        assert_eq!(
                            fields.get("test.answer", &note, &CancelToken::new()),
                            Some(Value::UInt(2))
                        );
                    }
                    release.send(()).unwrap();
                    assert_eq!(reading.join().unwrap(), Some(Value::UInt(1)));
                    for _ in 0..2 {
                        assert_eq!(
                            fields.get("test.answer", &note, &CancelToken::new()),
                            Some(Value::UInt(2))
                        );
                    }
                    assert_eq!(
                        reads.load(Ordering::SeqCst),
                        1,
                        "only the replacement's answer is remembered"
                    );
                });
            }
        }
    }

    #[cfg(unix)]
    #[test]
    fn memory_is_scoped_to_the_provider_selected_for_each_name() {
        let tmp = tempfile::tempdir().unwrap();
        let first = tmp.path().join("first.txt");
        let second = tmp.path().join("second.txt");
        std::fs::write(&first, "fixture").unwrap();
        std::fs::hard_link(&first, &second).unwrap();
        let fields = Fields::default();
        for (id, name, value) in [("first", "first.txt", 1), ("second", "second.txt", 2)] {
            fields.register(
                "test",
                Arc::new(Answer {
                    id,
                    version: 1,
                    value,
                    reads: Arc::default(),
                    name: Some(name),
                    fail: false,
                    wait: None,
                }),
            );
        }
        let cancel = CancelToken::new();
        // The file identity and field are the same, but a different
        // provider accepts each name. Their answers cannot share memory.
        assert_eq!(
            fields.get("test.answer", &first, &cancel),
            Some(Value::UInt(1))
        );
        assert_eq!(
            fields.get("test.answer", &second, &cancel),
            Some(Value::UInt(2))
        );
        assert_eq!(
            fields.get("test.answer", &first, &cancel),
            Some(Value::UInt(1))
        );
    }

    #[test]
    fn a_failed_read_never_becomes_persistent_absence_even_after_disable() {
        for withdrawn in [false, true] {
            let tmp = tempfile::tempdir().unwrap();
            let cache = crate::cache::Cache::new(tmp.path().join("cache"));
            let fields = Fields::new(cache.namespace("fields"));
            let note = tmp.path().join("note.txt");
            std::fs::write(&note, "fixture").unwrap();
            let reads = Arc::new(AtomicUsize::new(0));
            let (started, entered) = crossbeam_channel::bounded(1);
            let (resume, waiting) = crossbeam_channel::bounded(0);
            fields.register(
                "test",
                Arc::new(Answer {
                    id: "same",
                    version: 1,
                    value: 1,
                    reads: reads.clone(),
                    name: None,
                    fail: true,
                    wait: Some((started, waiting)),
                }),
            );
            std::thread::scope(|scope| {
                let reading = scope.spawn(|| fields.get("test.answer", &note, &CancelToken::new()));
                let release = resume;
                entered
                    .recv_timeout(std::time::Duration::from_secs(10))
                    .unwrap();
                if withdrawn {
                    fields.unregister_owner("test");
                }
                release.send(()).unwrap();
                assert_eq!(reading.join().unwrap(), None);
            });
            // Reopening the same cache is stronger than a warm memory hit:
            // a stopped helper has not said anything about the file's value.
            let again = Fields::new(cache.namespace("fields"));
            again.register(
                "test",
                Arc::new(Answer {
                    id: "same",
                    version: 1,
                    value: 2,
                    reads: reads.clone(),
                    name: None,
                    fail: false,
                    wait: None,
                }),
            );
            assert_eq!(
                again.get("test.answer", &note, &CancelToken::new()),
                Some(Value::UInt(2))
            );
            assert_eq!(reads.load(Ordering::SeqCst), 2);
        }
    }

    #[test]
    fn an_error_cached_by_the_old_format_is_read_again() {
        let tmp = tempfile::tempdir().unwrap();
        let cache = crate::cache::Cache::new(tmp.path().join("cache"));
        let slot = cache.namespace("fields");
        let note = tmp.path().join("note.txt");
        std::fs::write(&note, "fixture").unwrap();
        let (file, len, modified) = identity_of(&note).unwrap();
        // The original on-disk key, before failed reads became retryable.
        let old_key = [
            b"test.answer:same:1:".as_slice(),
            &file.kept_as(&note),
            format!(":{len}:{modified}").as_bytes(),
        ]
        .concat();
        slot.put(&old_key, b"null").unwrap();
        let fields = Fields::new(slot);
        let reads = Arc::new(AtomicUsize::new(0));
        fields.register(
            "test",
            Arc::new(Answer {
                id: "same",
                version: 1,
                value: 2,
                reads: reads.clone(),
                name: None,
                fail: false,
                wait: None,
            }),
        );
        assert_eq!(
            fields.get("test.answer", &note, &CancelToken::new()),
            Some(Value::UInt(2))
        );
        assert_eq!(reads.load(Ordering::SeqCst), 1);
    }

    /// Answers `size.words` with the file's text split in words, counting
    /// its reads.
    struct Words(Arc<AtomicUsize>);
    impl FieldProvider for Words {
        fn id(&self) -> &'static str {
            "words"
        }
        fn keys(&self) -> &'static [&'static str] {
            &["test.words"]
        }
        fn accepts(&self, _: &str, name: &OsStr) -> bool {
            name.to_string_lossy().ends_with(".txt")
        }
        fn read(&self, _: &str, path: &Path, _: &CancelToken) -> anyhow::Result<Option<Value>> {
            self.0.fetch_add(1, Ordering::SeqCst);
            let text = std::fs::read_to_string(path)?;
            let words = text.split_whitespace().count();
            Ok((words > 0).then_some(Value::UInt(words as u64)))
        }
    }

    #[test]
    fn a_field_is_read_once_until_its_file_changes() {
        let tmp = tempfile::tempdir().unwrap();
        let fields =
            Fields::new(crate::cache::Cache::new(tmp.path().join("cache")).namespace("fields"));
        let reads = Arc::new(AtomicUsize::new(0));
        fields.register("test", Arc::new(Words(Arc::clone(&reads))));
        let note = tmp.path().join("note one.txt");
        std::fs::write(&note, "two words").unwrap();
        let cancel = CancelToken::new();
        assert_eq!(
            fields.get("test.words", &note, &cancel),
            Some(Value::UInt(2))
        );
        assert_eq!(
            fields.get("test.words", &note, &cancel),
            Some(Value::UInt(2))
        );
        assert_eq!(reads.load(Ordering::SeqCst), 1);
        // Another size: another identity, read again.
        std::fs::write(&note, "now three words").unwrap();
        assert_eq!(
            fields.get("test.words", &note, &cancel),
            Some(Value::UInt(3))
        );
        assert_eq!(reads.load(Ordering::SeqCst), 2);
        // Nothing is an answer too, and kept.
        let empty = tmp.path().join("empty.txt");
        std::fs::write(&empty, "").unwrap();
        assert_eq!(fields.get("test.words", &empty, &cancel), None);
        assert_eq!(fields.get("test.words", &empty, &cancel), None);
        assert_eq!(reads.load(Ordering::SeqCst), 3);
        // Kept on disk: another registry over the same cache reads nothing.
        let again =
            Fields::new(crate::cache::Cache::new(tmp.path().join("cache")).namespace("fields"));
        again.register("test", Arc::new(Words(Arc::clone(&reads))));
        assert_eq!(
            again.get("test.words", &note, &cancel),
            Some(Value::UInt(3))
        );
        assert_eq!(reads.load(Ordering::SeqCst), 3);
    }

    /// Answers `test.first` and `test.last`, the file's first and last
    /// word, from one read, counting its reads.
    struct Ends(Arc<AtomicUsize>);
    impl FieldProvider for Ends {
        fn id(&self) -> &'static str {
            "ends"
        }
        fn keys(&self) -> &'static [&'static str] {
            &["test.first", "test.last"]
        }
        fn accepts(&self, _: &str, name: &OsStr) -> bool {
            name.to_string_lossy().ends_with(".txt")
        }
        fn read(
            &self,
            key: &str,
            path: &Path,
            cancel: &CancelToken,
        ) -> anyhow::Result<Option<Value>> {
            self.read_keys(&[key], path, cancel).pop().unwrap()
        }
        fn read_keys(
            &self,
            keys: &[&str],
            path: &Path,
            _: &CancelToken,
        ) -> Vec<anyhow::Result<Option<Value>>> {
            self.0.fetch_add(1, Ordering::SeqCst);
            let text = std::fs::read_to_string(path).unwrap_or_default();
            let words: Vec<&str> = text.split_whitespace().collect();
            keys.iter()
                .map(|key| {
                    let word = if *key == "test.first" {
                        words.first()
                    } else {
                        words.last()
                    };
                    Ok(word.map(|w| Value::Text((*w).into())))
                })
                .collect()
        }
    }

    #[test]
    fn keys_of_one_provider_are_read_together_and_a_known_file_is_not_stat_ed() {
        let tmp = tempfile::tempdir().unwrap();
        let fields =
            Fields::new(crate::cache::Cache::new(tmp.path().join("cache")).namespace("fields"));
        let reads = Arc::new(AtomicUsize::new(0));
        fields.register("test", Arc::new(Ends(Arc::clone(&reads))));
        let note = tmp.path().join("it's a note.txt");
        std::fs::write(&note, "first then last").unwrap();
        let cancel = CancelToken::new();
        let text = |w: &str| Some(Value::Text(w.into()));
        let both = ["test.first", "test.last", "test.unknown"];
        assert_eq!(
            fields.get_all(&both, &note, None, &cancel),
            [text("first"), text("last"), None]
        );
        assert_eq!(reads.load(Ordering::SeqCst), 1, "one read for both");
        // Its identity said, the file is neither stat-ed nor read again:
        // gone from where it was, it is still answered.
        let meta = std::fs::metadata(&note).unwrap();
        let id = crate::fs::stat::identity(&note, &meta).unwrap();
        let modified = meta
            .modified()
            .unwrap()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let known = (id, meta.len(), u64::try_from(modified).unwrap());
        let moved = tmp.path().join("moved.txt");
        std::fs::rename(&note, &moved).unwrap();
        assert_eq!(
            fields.get_all(&["test.last"], &note, Some(known), &cancel),
            [text("last")]
        );
        assert_eq!(reads.load(Ordering::SeqCst), 1);
        assert_eq!(
            fields.get("test.first", &note, &cancel),
            None,
            "stat-ed: gone"
        );
    }

    #[test]
    fn a_file_whose_number_a_mount_made_up_is_kept_on_disk_by_its_path() {
        let words = Words(Arc::new(AtomicUsize::new(0)));
        let path = Path::new("/srv/it's here.txt");
        let lasting = crate::entry::FileId::new(5, 7);
        assert_eq!(
            disk_key(
                "test.words",
                (words.id(), words.version()),
                (lasting, 3, 9),
                path
            ),
            format!("2:test.words:words:{}:5:7:3:9", words.version()).into_bytes(),
            "cache format and provider version precede the stable file identity"
        );
        crate::fs::reach::pretend_made_up(6, 1 << 40);
        let made_up = crate::entry::FileId::new(6, 7);
        // The same file mounted again, numbered anew.
        let other = crate::entry::FileId::new(6, 99);
        assert_eq!(
            disk_key(
                "test.words",
                (words.id(), words.version()),
                (made_up, 3, 9),
                path
            ),
            disk_key(
                "test.words",
                (words.id(), words.version()),
                (other, 3, 9),
                path
            ),
            "a mount's number is no longer known once it is let go of"
        );
        assert_ne!(
            disk_key(
                "test.words",
                (words.id(), words.version()),
                (made_up, 3, 9),
                path
            ),
            disk_key(
                "test.words",
                (words.id(), words.version()),
                (made_up, 3, 9),
                Path::new("/srv/other")
            ),
        );
    }

    #[test]
    fn only_files_a_provider_accepts_are_asked_and_its_owner_takes_it_away() {
        let tmp = tempfile::tempdir().unwrap();
        let fields = Fields::new(crate::cache::Cache::new(tmp.path()).namespace("fields"));
        let reads = Arc::new(AtomicUsize::new(0));
        fields.register("test", Arc::new(Words(Arc::clone(&reads))));
        let picture = tmp.path().join("picture.jpg");
        std::fs::write(&picture, "not words").unwrap();
        let cancel = CancelToken::new();
        assert_eq!(fields.get("test.words", &picture, &cancel), None);
        assert_eq!(reads.load(Ordering::SeqCst), 0);
        assert_eq!(fields.keys_for(OsStr::new("a.txt")), ["test.words"]);
        assert!(fields.keys_for(OsStr::new("a.jpg")).is_empty());
        assert!(fields.knows("test.words"));
        let revision = fields.revision();
        fields.unregister_owner("test");
        assert!(!fields.knows("test.words"));
        assert!(fields.revision() > revision);
    }

    // Pipes are made with mknodat, Linux's alone here (ADR-035).
    #[cfg(target_os = "linux")]
    #[test]
    fn a_link_is_asked_as_what_it_leads_to() {
        let tmp = tempfile::tempdir().unwrap();
        let fields = Fields::new(crate::cache::Cache::new(tmp.path()).namespace("fields"));
        let reads = Arc::new(AtomicUsize::new(0));
        fields.register("test", Arc::new(Words(Arc::clone(&reads))));
        std::fs::write(tmp.path().join("it's a note.txt"), "three short words").unwrap();
        // Neither link's name says text: what they lead to does.
        let link = tmp.path().join("a link");
        std::os::unix::fs::symlink("it's a note.txt", &link).unwrap();
        let chain = tmp.path().join("a link to a link");
        std::os::unix::fs::symlink("a link", &chain).unwrap();
        let cancel = CancelToken::new();
        assert_eq!(
            fields.get("test.words", &link, &cancel),
            Some(Value::UInt(3))
        );
        assert_eq!(
            fields.get("test.words", &chain, &cancel),
            Some(Value::UInt(3))
        );
        // One file behind both links: read once.
        assert_eq!(reads.load(Ordering::SeqCst), 1);
        // A link that leads nowhere has nothing to read.
        let broken = tmp.path().join("broken.txt");
        std::os::unix::fs::symlink("nowhere", &broken).unwrap();
        assert_eq!(crate::fs::content_path(&broken), None);
        assert_eq!(fields.get("test.words", &broken, &cancel), None);
        // Nor does a pipe, whatever its name: it is never opened.
        let pipe = tmp.path().join("a pipe.txt");
        rustix::fs::mknodat(
            rustix::fs::CWD,
            &pipe,
            rustix::fs::FileType::Fifo,
            rustix::fs::Mode::from_raw_mode(0o600),
            0,
        )
        .unwrap();
        assert_eq!(crate::fs::content_path(&pipe), None);
        assert_eq!(fields.get("test.words", &pipe, &cancel), None);
        assert_eq!(reads.load(Ordering::SeqCst), 1);
    }
}
