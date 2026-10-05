//! The throwaway cache: content-addressed, namespaced per plugin, deletable
//! at any time with no loss beyond recomputation.

use std::path::{Path, PathBuf};

/// Root of the cache directory.
#[derive(Clone, Debug)]
pub struct Cache {
    root: PathBuf,
}

impl Cache {
    /// The platform cache dir (`$XDG_CACHE_HOME/bibli` on Linux).
    pub fn default_root() -> PathBuf {
        crate::xdg::cache_home()
            .unwrap_or_else(std::env::temp_dir)
            .join("bibli")
    }

    pub fn new(root: impl Into<PathBuf>) -> Self {
        Cache { root: root.into() }
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    /// A namespaced slot. `namespace` is the plugin id.
    pub fn namespace(&self, namespace: &str) -> CacheSlot {
        CacheSlot {
            dir: self.root.join(namespace),
        }
    }

    /// Delete everything. Always safe.
    pub fn clear(&self) -> std::io::Result<()> {
        match std::fs::remove_dir_all(&self.root) {
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
            r => r,
        }
    }
}

/// One plugin's corner of the cache. Keys are hashed; values are files.
#[derive(Clone, Debug)]
pub struct CacheSlot {
    dir: PathBuf,
}

impl CacheSlot {
    /// Bound this namespace's disk usage by removing its oldest files.
    /// Worker-only maintenance; never call from render or command dispatch.
    /// Another process may be writing and trimming the same cache: a file
    /// or bucket that vanishes mid-walk is skipped, not an error.
    pub fn trim_to_bytes(&self, budget: u64) -> std::io::Result<()> {
        let mut files = Vec::new();
        let mut total = 0u64;
        let buckets = match std::fs::read_dir(&self.dir) {
            Ok(buckets) => buckets,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(()),
            Err(e) => return Err(e),
        };
        for bucket in buckets.flatten() {
            if !bucket.file_type().is_ok_and(|t| t.is_dir()) {
                continue;
            }
            let Ok(entries) = std::fs::read_dir(bucket.path()) else {
                continue;
            };
            for file in entries.flatten() {
                let Ok(metadata) = file.metadata() else {
                    continue;
                };
                if !metadata.is_file() {
                    continue;
                }
                let Ok(modified) = metadata.modified() else {
                    continue;
                };
                total = total.saturating_add(metadata.len());
                files.push((modified, metadata.len(), file.path()));
            }
        }
        files.sort_by_key(|(time, _, _)| *time);
        for (_, bytes, path) in files {
            if total <= budget {
                break;
            }
            if std::fs::remove_file(path).is_ok() {
                total = total.saturating_sub(bytes);
            }
        }
        Ok(())
    }

    fn path_for(&self, key: &[u8]) -> PathBuf {
        let h = blake3::hash(key).to_hex();
        self.dir.join(&h[..2]).join(&h[2..])
    }

    pub fn get(&self, key: &[u8]) -> Option<Vec<u8>> {
        std::fs::read(self.path_for(key)).ok()
    }

    /// Write atomically (a uniquely named temp file, then a rename) so
    /// readers never see a partial value and concurrent writers never share
    /// a temp file.
    pub fn put(&self, key: &[u8], value: &[u8]) -> std::io::Result<()> {
        use std::io::Write as _;
        let path = self.path_for(key);
        let parent = path.parent().expect("cache path has parent");
        std::fs::create_dir_all(parent)?;
        let mut tmp = tempfile::Builder::new()
            .prefix(".tmp-")
            .tempfile_in(parent)?;
        tmp.write_all(value)?;
        tmp.persist(path).map(drop).map_err(|e| e.error)
    }

    pub fn remove(&self, key: &[u8]) -> std::io::Result<()> {
        match std::fs::remove_file(self.path_for(key)) {
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
            r => r,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pruning_bounds_one_namespace_without_touching_another() {
        let root = tempfile::tempdir().unwrap();
        let cache = Cache::new(root.path());
        let previews = cache.namespace("previews");
        let other = cache.namespace("other");
        other.put(b"keep", b"keep").unwrap();
        previews.put(b"first", &[1; 100]).unwrap();
        previews.put(b"second", &[2; 100]).unwrap();
        previews.trim_to_bytes(100).unwrap();
        let retained = [b"first".as_slice(), b"second".as_slice()]
            .iter()
            .filter_map(|key| previews.get(key))
            .map(|v| v.len())
            .sum::<usize>();
        assert!(retained <= 100);
        assert_eq!(other.get(b"keep").unwrap(), b"keep");
        previews.trim_to_bytes(0).unwrap();
        assert!(previews.get(b"first").is_none() && previews.get(b"second").is_none());
    }
}
