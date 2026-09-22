//! Icon lookup through the platform crate, cached per (name, size), and a
//! GPUI `AssetSource` serving absolute filesystem paths so `svg()` works.

use std::borrow::Cow;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use bibli_core::Entry;
use bibli_platform::{icons, mime};
use gpui::{AnyElement, AssetSource, SharedString, img, prelude::*, px, svg};

/// Serves any absolute path on disk. Icon themes live outside the binary.
pub struct FsAssets;

impl AssetSource for FsAssets {
    fn load(&self, path: &str) -> anyhow::Result<Option<Cow<'static, [u8]>>> {
        if let Some(bytes) = crate::toolbar::asset(path) {
            return Ok(Some(Cow::Borrowed(bytes)));
        }
        if !Path::new(path).is_absolute() {
            return Ok(None);
        }
        match std::fs::read(path) {
            Ok(bytes) => Ok(Some(bytes.into())),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(e) => Err(e.into()),
        }
    }

    fn list(&self, path: &str) -> anyhow::Result<Vec<SharedString>> {
        Ok(std::fs::read_dir(path)?
            .filter_map(|e| Some(e.ok()?.path().to_string_lossy().into_owned().into()))
            .collect())
    }
}

#[derive(Default)]
pub struct IconCache {
    by_mime: HashMap<(String, u16), Option<Arc<Path>>>,
    folder: HashMap<u16, Option<Arc<Path>>>,
    named: HashMap<(String, u16), Option<Arc<Path>>>,
}

impl IconCache {
    /// An icon of the desktop's theme by name (`user-home`, `drive-harddisk`).
    pub fn named(&mut self, name: &str, size: u16) -> Option<Arc<Path>> {
        self.named
            .entry((name.to_owned(), size))
            .or_insert_with(|| icons::lookup(name, size).map(PathBuf::into))
            .clone()
    }

    /// Icon path for an entry: the folder icon or the MIME type's icon.
    pub fn for_entry(&mut self, e: &Entry, size: u16) -> Option<Arc<Path>> {
        if e.is_dir() {
            return self
                .folder
                .entry(size)
                .or_insert_with(|| icons::folder(size).map(PathBuf::into))
                .clone();
        }
        self.for_name(&e.name, size)
    }

    /// The icon of a file's MIME type, by its name; the unknown-file icon
    /// when the name says nothing.
    pub fn for_name(&mut self, name: &std::ffi::OsStr, size: u16) -> Option<Arc<Path>> {
        let m = mime::by_name(name);
        let key = m.as_ref().map_or("", mime::Mime::as_str).to_owned();
        self.by_mime
            .entry((key, size))
            .or_insert_with(|| {
                m.map_or_else(|| icons::unknown(size), |m| icons::for_mime(&m, size))
                    .map(PathBuf::into)
            })
            .clone()
    }
}

/// Render an icon path at `size` px. SVGs go through `img()` too, which
/// rasterizes them in full color; `svg()` would render a monochrome mask.
pub fn element(path: &Path, size: f32) -> AnyElement {
    let _ = svg; // kept importable for symbolic icons
    img(path.to_path_buf())
        .size(px(size))
        .flex_none()
        .into_any_element()
}
