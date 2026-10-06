//! Icon theme lookup. Cached; never touches the network.
//!
//! A lookup that misses the cache reads the icon theme from disk, through up
//! to four themes, and the first one also lists every installed theme: it
//! blocks, so it belongs on a worker, never in a render. The UI asks
//! [`try_cached`] while drawing and runs [`lookup`] (or [`for_mime`],
//! [`folder`], [`unknown`], each a few lookups) on a worker for what it
//! did not have; a later [`try_cached`] then answers.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Mutex, OnceLock};

use crate::mime::Mime;

/// An SVG icon (or `.svgz`) drawn within `side` pixels on its longer side,
/// the size it is shown at, in RGBA: an icon theme's full-color SVG
/// declares a size of its own (Adwaita's 128 px), which has nothing to do
/// with the 16 px of a sidebar row. Inert, as a drawing previewed is.
/// Reads the file: call it from a worker.
pub fn rasterize(path: &std::path::Path, side: u32) -> anyhow::Result<crate::inspection::Decoded> {
    let bytes = crate::inspection::read_drawing(path, &bibli_core::CancelToken::new())?;
    crate::inspection::drawing_within(&bytes, side)
}

/// The active icon theme name. Lists the installed themes the first time:
/// blocks.
pub fn theme_name() -> String {
    static NAME: OnceLock<String> = OnceLock::new();
    NAME.get_or_init(imp::detect_theme).clone()
}

fn cache() -> &'static Mutex<HashMap<(String, u16), Option<PathBuf>>> {
    static CACHE: OnceLock<Mutex<HashMap<(String, u16), Option<PathBuf>>>> = OnceLock::new();
    CACHE.get_or_init(|| Mutex::new(HashMap::new()))
}

/// What [`lookup`] found for `name` at `size`, if it has already run:
/// `None` when it has not, `Some(None)` when the themes have no such icon.
/// Never touches the filesystem, so it is safe in a render.
pub fn try_cached(name: &str, size: u16) -> Option<Option<PathBuf>> {
    cache().lock().ok()?.get(&(name.to_owned(), size)).cloned()
}

/// Find `name` at `size` px in the active theme (with fallbacks).
/// Returns a `.png` or `.svg` path. Reads the theme on a cache miss: blocks,
/// call it from a worker.
pub fn lookup(name: &str, size: u16) -> Option<PathBuf> {
    let key = (name.to_owned(), size);
    if let Some(hit) = cache().lock().ok()?.get(&key) {
        return hit.clone();
    }
    let found = imp::lookup(name, size);
    if let Ok(mut c) = cache().lock() {
        c.insert(key, found.clone());
    }
    found
}

/// Icon for a MIME type: specific, then generic, then a last resort.
/// Blocks like [`lookup`].
pub fn for_mime(mime: &Mime, size: u16) -> Option<PathBuf> {
    lookup(&mime.icon_name(), size)
        .or_else(|| lookup(&mime.generic_icon_name(), size))
        .or_else(|| {
            lookup(
                if mime.top_level() == "text" {
                    "text-x-generic"
                } else {
                    "application-x-generic"
                },
                size,
            )
        })
        .or_else(|| lookup("text-x-generic", size))
}

/// Icon for a file whose name says nothing (`mime::by_name` gave `None`):
/// what the desktop draws for its unknown type. Only a picture; what the
/// file holds is `mime::by_content`'s to say.
pub fn unknown(size: u16) -> Option<PathBuf> {
    for_mime(&Mime("application/octet-stream".into()), size)
}

pub fn folder(size: u16) -> Option<PathBuf> {
    lookup("folder", size).or_else(|| lookup("inode-directory", size))
}

#[cfg(target_os = "linux")]
mod imp {
    use std::path::PathBuf;

    fn gtk_setting(file: &str) -> Option<String> {
        let path = crate::xdg::config_home()?.join(file);
        let text = std::fs::read_to_string(path).ok()?;
        crate::keyfile::entries(&text)
            .find(|(group, key, _)| *group == "Settings" && *key == "gtk-icon-theme-name")
            .map(|(_, _, value)| value.trim().to_owned())
            .filter(|s| !s.is_empty())
    }

    pub fn detect_theme() -> String {
        let installed = freedesktop_icons::list_themes();
        let configured =
            gtk_setting("gtk-4.0/settings.ini").or_else(|| gtk_setting("gtk-3.0/settings.ini"));
        for candidate in configured
            .iter()
            .map(String::as_str)
            .chain(["Adwaita", "breeze", "Papirus"])
        {
            if installed.iter().any(|t| t == candidate) {
                return candidate.to_owned();
            }
        }
        "hicolor".to_owned()
    }

    pub fn lookup(name: &str, size: u16) -> Option<PathBuf> {
        let theme = super::theme_name();
        let mut candidates: Vec<&str> = vec![&theme];
        for fallback in ["Adwaita", "breeze", "hicolor"] {
            if fallback != theme {
                candidates.push(fallback);
            }
        }
        candidates.into_iter().find_map(|t| {
            freedesktop_icons::lookup(name)
                .with_size(size)
                .with_theme(t)
                .find()
        })
    }
}

#[cfg(not(target_os = "linux"))]
mod imp {
    use std::path::PathBuf;

    pub fn detect_theme() -> String {
        String::new()
    }
    pub fn lookup(_name: &str, _size: u16) -> Option<PathBuf> {
        None
    }
}

#[cfg(test)]
mod rasterize_tests {
    use super::rasterize;

    /// A drawing as an icon theme's: `width` by `height` px, filled red.
    fn icon(width: u32, height: u32) -> String {
        format!(
            r##"<svg xmlns="http://www.w3.org/2000/svg" width="{width}" height="{height}" viewBox="0 0 {width} {height}"><rect width="{width}" height="{height}" fill="#ff0000"/></svg>"##
        )
    }

    #[test]
    fn an_icon_is_drawn_at_the_size_it_is_shown_not_the_one_it_declares() {
        let dir = tempfile::tempdir().unwrap();
        let square = dir.path().join("user-trash.svg");
        std::fs::write(&square, icon(128, 128)).unwrap();
        let drawn = rasterize(&square, 32).unwrap();
        assert_eq!((drawn.width, drawn.height), (32, 32));
        assert_eq!(drawn.own, (128, 128));
        assert_eq!(drawn.rgba.len(), 32 * 32 * 4);
        let middle = ((16 * 32 + 16) * 4) as usize;
        assert_eq!(&drawn.rgba[middle..middle + 4], &[255, 0, 0, 255]);

        // A wide one keeps its shape within the side.
        let wide = dir.path().join("wide.svg");
        std::fs::write(&wide, icon(128, 64)).unwrap();
        let drawn = rasterize(&wide, 32).unwrap();
        assert_eq!((drawn.width, drawn.height), (32, 16));
    }

    #[test]
    fn a_compressed_icon_is_drawn_too() {
        use std::io::Write as _;
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("folder.svgz");
        let mut gz = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
        gz.write_all(icon(64, 64).as_bytes()).unwrap();
        std::fs::write(&path, gz.finish().unwrap()).unwrap();
        let drawn = rasterize(&path, 24).unwrap();
        assert_eq!((drawn.width, drawn.height), (24, 24));
    }
}

// The icon themes are the free desktops' (ADR-035).
#[cfg(all(test, target_os = "linux"))]
mod tests {
    use super::*;

    #[test]
    fn folder_resolves_and_is_cached() {
        let p = folder(48).expect("an icon theme with a folder icon is installed");
        let ext = p.extension().and_then(|e| e.to_str()).unwrap_or("");
        assert!(
            ext == "png" || ext == "svg",
            "unexpected icon format: {}",
            p.display()
        );
        assert_eq!(lookup("folder", 48), Some(p.clone()));
        assert_eq!(try_cached("folder", 48), Some(Some(p)));
        assert_eq!(try_cached("folder", 47), None, "never looked up");
        assert!(
            cache()
                .lock()
                .unwrap()
                .contains_key(&("folder".to_owned(), 48))
        );
    }

    #[test]
    fn mime_icon_falls_back() {
        let m = Mime("application/x-bibli-nonexistent".into());
        // Either the generic or last-resort icon must exist.
        assert!(for_mime(&m, 32).is_some());
    }
}
