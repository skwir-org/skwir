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
