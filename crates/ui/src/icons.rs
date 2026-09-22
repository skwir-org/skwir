//! Icon lookup through the platform crate, resolved on a worker and cached
//! per (name, size), and a GPUI `AssetSource` serving absolute filesystem paths so `svg()` works.

use std::borrow::Cow;
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use bibli_core::{App, Entry};
use bibli_platform::{icons, mime};
use gpui::{AnyElement, AssetSource, SharedString, img, prelude::*, px};

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

/// What an icon is looked up by.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
enum Want {
    /// A theme icon name (`user-home`).
    Named(String),
    /// A MIME type's icon; the empty type is the unknown file.
    Mime(String),
    Folder,
}

type Resolved = Vec<((Want, u16), Option<PathBuf>)>;

/// Icons of the desktop's theme, per size. Looking one up reads the theme
/// from disk, so a miss is answered `None` for now and resolved on a worker
/// with the others this frame missed (`request`); `absorb` takes the
/// answers and the next frame draws them. Drawing only ever reads this map.
#[derive(Default)]
pub struct IconCache {
    /// Answers, per size then per name or type.
    known: HashMap<u16, HashMap<Want, Option<Arc<Path>>>>,
    wanted: HashSet<(Want, u16)>,
    results: Arc<Mutex<Resolved>>,
}

impl IconCache {
    fn lookup(&mut self, want: Want, size: u16) -> Option<Arc<Path>> {
        if let Some(found) = self.known.get(&size).and_then(|known| known.get(&want)) {
            return found.clone();
        }
        // Resolved by an earlier worker, or by anyone else who asked.
        let cached = match &want {
            Want::Named(name) => icons::try_cached(name, size),
            _ => None,
        };
        let Some(found) = cached else {
            self.wanted.insert((want, size));
            return None;
        };
        let found: Option<Arc<Path>> = found.map(PathBuf::into);
        self.known
            .entry(size)
            .or_default()
            .insert(want, found.clone());
        found
    }

    /// An icon of the desktop's theme by name (`user-home`, `drive-harddisk`).
    pub fn named(&mut self, name: &str, size: u16) -> Option<Arc<Path>> {
        self.lookup(Want::Named(name.to_owned()), size)
    }

    /// Icon path for an entry: the folder icon or the MIME type's icon.
    pub fn for_entry(&mut self, e: &Entry, size: u16) -> Option<Arc<Path>> {
        if e.is_dir() {
            return self.folder(size);
        }
        self.for_name(&e.name, size)
    }

    pub fn folder(&mut self, size: u16) -> Option<Arc<Path>> {
        self.lookup(Want::Folder, size)
    }

    /// The icon of a file's MIME type, by its name; the unknown-file icon
    /// when the name says nothing.
    pub fn for_name(&mut self, name: &std::ffi::OsStr, size: u16) -> Option<Arc<Path>> {
        self.for_mime(mime::by_name(name).as_ref(), size)
    }

    /// The icon of a MIME type already known; `None` is the unknown file.
    pub fn for_mime(&mut self, mime: Option<&mime::Mime>, size: u16) -> Option<Arc<Path>> {
        let key = mime.map_or_else(String::new, |m| m.0.clone());
        self.lookup(Want::Mime(key), size)
    }

    /// Resolve what this frame missed, in one job. Call after drawing.
    pub fn request(&mut self, app: &Arc<App>) {
        if self.wanted.is_empty() {
            return;
        }
        let wanted: Vec<(Want, u16)> = self.wanted.drain().collect();
        // In flight: read as no icon until the answer, and not asked again.
        for (want, size) in &wanted {
            self.known
                .entry(*size)
                .or_default()
                .insert(want.clone(), None);
        }
        let results = Arc::clone(&self.results);
        let weak = app.weak();
        app.scheduler.spawn(
            bibli_core::scheduler::Priority::Visible,
            "icons",
            app.lifetime().child(),
            move || {
                let resolved: Resolved = wanted
                    .into_iter()
                    .map(|(want, size)| {
                        let path = match &want {
                            Want::Named(name) => icons::lookup(name, size),
                            Want::Folder => icons::folder(size),
                            Want::Mime(m) if m.is_empty() => icons::unknown(size),
                            Want::Mime(m) => icons::for_mime(&mime::Mime(m.clone()), size),
                        };
                        ((want, size), path)
                    })
                    .collect();
                if let Ok(mut slot) = results.lock() {
                    slot.extend(resolved);
                }
                if let Some(app) = weak.upgrade() {
                    app.wake();
                }
            },
        );
    }

    /// Take finished lookups. Returns whether an icon arrived.
    pub fn absorb(&mut self) -> bool {
        let Ok(mut slot) = self.results.lock() else {
            return false;
        };
        let found = std::mem::take(&mut *slot);
        drop(slot);
        let changed = found.iter().any(|(_, path)| path.is_some());
        for ((want, size), path) in found {
            self.known
                .entry(size)
                .or_default()
                .insert(want, path.map(PathBuf::into));
        }
        changed
    }
}

/// Render an icon path at `size` px. SVGs go through `img()` too, which
/// rasterizes them in full color; `svg()` would render a monochrome mask.
pub fn element(path: &Path, size: f32) -> AnyElement {
    img(path.to_path_buf())
        .size(px(size))
        .flex_none()
        .into_any_element()
}
