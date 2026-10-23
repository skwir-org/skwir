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

    /// The resolved file to read or bind, and its decoded relative name.
    /// Decode once before checking lexical traversal and links outside
    /// the model's folder. A dangling link grants no access.
    pub(super) fn resolve(&self, uri: &str) -> Option<(PathBuf, PathBuf)> {
        // Embedded data is read by the worker separately; no scheme is
        // a filesystem path, even if it happens to be percent-encoded.
        if uri.contains(':') {
            return None;
        }
        let decoded = decode(uri)?;
        let relative = Path::new(&decoded);
        if decoded.contains(':')
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

/// glTF permits UTF-8 names as IRIs or percent-encoded URIs. Every `%`
/// names exactly two hex digits; `%25` names a literal percent, which is
/// never decoded again. `+` is a filename character, not a form's space.
fn decode(uri: &str) -> Option<String> {
    let hex = |byte| match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        b'A'..=b'F' => Some(byte - b'A' + 10),
        _ => None,
    };
    let mut bytes = uri.bytes();
    let mut decoded = Vec::with_capacity(uri.len());
    while let Some(byte) = bytes.next() {
        let byte = if byte == b'%' {
            hex(bytes.next()?)? * 16 + hex(bytes.next()?)?
        } else {
            byte
        };
        if byte == 0 {
            return None;
        }
        decoded.push(byte);
    }
    String::from_utf8(decoded).ok()
}
