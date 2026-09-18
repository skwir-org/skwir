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
        dirs::cache_dir()
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
    pub fn trim_to_bytes(&self, budget: u64) -> std::io::Result<()> {
        let mut files = Vec::new();
        let mut total = 0u64;
        if !self.dir.exists() {
            return Ok(());
        }
        for bucket in std::fs::read_dir(&self.dir)? {
            let bucket = bucket?;
            if !bucket.file_type()?.is_dir() {
                continue;
            }
            for file in std::fs::read_dir(bucket.path())? {
                let file = file?;
                if !file.file_type()?.is_file() {
                    continue;
                }
                let metadata = file.metadata()?;
                total = total.saturating_add(metadata.len());
                files.push((metadata.modified()?, metadata.len(), file.path()));
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

    /// Write atomically (temp file + rename) so readers never see a partial value.
    pub fn put(&self, key: &[u8], value: &[u8]) -> std::io::Result<()> {
        let path = self.path_for(key);
        let parent = path.parent().expect("cache path has parent");
        std::fs::create_dir_all(parent)?;
        let tmp = parent.join(format!(
            ".tmp-{}-{}",
            std::process::id(),
            blake3::hash(value).to_hex()
        ));
        std::fs::write(&tmp, value)?;
        std::fs::rename(tmp, path)
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
