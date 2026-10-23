//! The glTF buffer boundary shared by the host's sandbox inputs and the
//! model worker's reads. Kept std-only so the isolated worker can compile
//! this policy without linking the platform and its document decoders.

use std::path::{Component, Path, PathBuf};

/// A model's actual folder, also when a sandbox reaches it through a link.
pub(super) struct Folder(PathBuf);

impl Folder {
    pub(super) fn of_model(model: &Path) -> Option<Self> {
        Some(Self(model.canonicalize().ok()?.parent()?.to_owned()))
    }

    /// The resolved file to read or bind, and the relative name the model
    /// uses. Both lexical traversal and links outside the model's folder
    /// are refused. A dangling link grants no access.
    pub(super) fn resolve(&self, uri: &str) -> Option<(PathBuf, PathBuf)> {
        let relative = Path::new(uri);
        if uri.contains(':')
            || !relative
                .components()
                .all(|component| matches!(component, Component::Normal(_)))
        {
            return None;
        }
        let resolved = self.0.join(relative).canonicalize().ok()?;
        if !resolved.starts_with(&self.0) || !resolved.is_file() {
            return None;
        }
        Some((resolved, relative.to_owned()))
    }
}
