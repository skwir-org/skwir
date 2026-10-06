//! Fields filled on demand (ADR-023): what a file says that is too costly
//! to read for every listing, read only when something asks. A picture's
//! position is the first: reading EXIF during hydration would slow every
//! folder for users who never search by place (ADR-020), but a search
//! with `near:` and Details want it.
//!
//! A plugin registers a [`FieldProvider`] for the keys it answers; a
//! caller on a worker asks [`Fields::get`] for a key of a file. The answer,
//! a value or nothing, is kept by the file's identity (device, inode,
//! size, modification time to the nanosecond) in memory and in the
//! deletable cache, so a file is read once until it changes. Nothing here
//! runs on the UI thread or per frame.
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
    /// The words Details labels `key` with; `None` keeps it out of
    /// Details (a position, which the picture's own facts show).
    fn label(&self, _key: &str) -> Option<&'static str> {
        None
    }
    /// Whom Details credits for `key`'s answers, when their data asks
    /// for it (a licence's attribution). Asked on a worker.
    fn credit(&self, _key: &str) -> Option<String> {
        None
    }
    /// Cheap, from the name alone: whether a file of this name may have
    /// `key`. Asked before anything is read.
    fn accepts(&self, key: &str, name: &OsStr) -> bool;
    /// What the file at `path` says of `key`, read now, on a worker.
    /// `Ok(None)` when it says nothing; an error is kept as nothing too.
    fn read(&self, key: &str, path: &Path, cancel: &CancelToken) -> anyhow::Result<Option<Value>>;
}

/// A file's identity: its answers hold while it is the same.
type Identity = (u64, u64, u64, u64);

#[derive(Default)]
pub struct Fields {
    providers: RwLock<Vec<(Arc<str>, Arc<dyn FieldProvider>)>>,
    /// `None` keeps answers in memory only (tests, tools).
    cache: Option<CacheSlot>,
    remembered: Mutex<HashMap<(Arc<str>, Identity), Option<Value>>>,
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
        self.providers.write().push((owner.into(), provider));
        self.changed();
    }

    pub fn unregister_owner(&self, owner: &str) {
        let before = self.providers.read().len();
        self.providers.write().retain(|(o, _)| &**o != owner);
        if self.providers.read().len() != before {
            self.changed();
        }
    }

    /// Advanced when providers come or go: what was asked may be answered
    /// otherwise now.
    pub fn revision(&self) -> u64 {
        self.revision.load(Ordering::Acquire)
    }

    fn changed(&self) {
        self.remembered.lock().clear();
        self.revision.fetch_add(1, Ordering::AcqRel);
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

    /// The keys shown in Details for a file named `name`, with their
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

    /// Whom Details credits for `key`, for a file named `name`. On a worker.
    pub fn credit(&self, key: &str, name: &OsStr) -> Option<String> {
        self.provider(key, name)?.credit(key)
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

    fn provider(&self, key: &str, name: &OsStr) -> Option<Arc<dyn FieldProvider>> {
        self.providers
            .read()
            .iter()
            .find(|(_, p)| p.keys().contains(&key) && p.accepts(key, name))
            .map(|(_, p)| Arc::clone(p))
    }

    /// The value of `key` for the file at `path`: remembered, kept in the
    /// cache, or read now. On a worker: it may stat and read the file.
    /// `None` when no provider answers it, the file says nothing, or it
    /// cannot be read.
    pub fn get(&self, key: &str, path: &Path, cancel: &CancelToken) -> Option<Value> {
        // A link is asked as what it leads to, which is what is read.
        let content = crate::fs::content_path(path)?;
        let provider = self.provider(key, content.file_name()?)?;
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
        let identity = (id.dev, id.ino, meta.len(), modified);
        let memory_key: (Arc<str>, Identity) = (key.into(), identity);
        if let Some(value) = self.remembered.lock().get(&memory_key) {
            return value.clone();
        }
        let disk_key = format!(
            "{key}:{}:{}:{}:{}:{}:{}",
            provider.id(),
            provider.version(),
            identity.0,
            identity.1,
            identity.2,
            identity.3
        );
        let kept = self
            .cache
            .as_ref()
            .and_then(|cache| cache.get(disk_key.as_bytes()))
            .and_then(|bytes| serde_json::from_slice::<Option<Value>>(&bytes).ok());
        let value = if let Some(value) = kept {
            value
        } else {
            {
                let value = match provider.read(key, path, cancel) {
                    Ok(value) => value,
                    Err(error) => {
                        tracing::debug!(%error, key, "field not read");
                        None
                    }
                };
                // A read cut short by a cancel says nothing of the file.
                if cancel.is_cancelled() {
                    return None;
                }
                if let Some(cache) = &self.cache
                    && let Ok(bytes) = serde_json::to_vec(&value)
                    && let Err(error) = cache.put(disk_key.as_bytes(), &bytes)
                {
                    tracing::debug!(%error, "field not kept");
                }
                value
            }
        };
        let mut remembered = self.remembered.lock();
        if remembered.len() >= REMEMBERED {
            remembered.clear();
        }
        remembered.insert(memory_key, value.clone());
        value
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::AtomicUsize;

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
