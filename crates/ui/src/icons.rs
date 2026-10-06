//! Icon lookup through the platform crate, resolved on a worker and cached
//! per (name, size), and a GPUI `AssetSource` serving absolute filesystem paths so `svg()` works.
//!
//! A named icon the theme has only as a symbolic variant (current Adwaita
//! and Breeze ship `folder-saved-search` only as `-symbolic`) is used as
//! GTK does, drawn in the text color: a symbolic SVG is a shape to tint,
//! not a picture. The worker that finds one reads it too, so drawing it
//! never touches the disk.
//!
//! A full-color SVG icon is rasterized at the size it is drawn, in device
//! pixels rounded up to a few sides (`raster_side`), never at the size its
//! file declares: Adwaita's are 128 px, which `img()` drew at 256 by 256
//! for a 16 px row, filters and all.

use std::borrow::Cow;
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use bibli_core::{App, Entry};
use bibli_platform::{icons, mime};
use gpui::{
    AnyElement, Asset, AssetSource, Hsla, ImageCacheError, ImageSource, RenderImage, SharedString,
    Window, div, img, prelude::*, px, svg,
};

/// Serves any absolute path on disk. Icon themes live outside the binary.
pub struct FsAssets;

/// Symbolic icons read by the lookup worker, by the path `svg()` asks for.
fn preloaded() -> &'static Mutex<HashMap<String, Arc<[u8]>>> {
    static PRELOADED: std::sync::OnceLock<Mutex<HashMap<String, Arc<[u8]>>>> =
        std::sync::OnceLock::new();
    PRELOADED.get_or_init(Mutex::default)
}

/// Whether `path` is a symbolic icon: a shape drawn in the text color.
pub fn is_symbolic(path: &Path) -> bool {
    path.file_stem()
        .and_then(|stem| stem.to_str())
        .is_some_and(|stem| stem.ends_with("-symbolic"))
}

/// A named icon, or its symbolic variant when the theme has only that.
/// Blocks: call it from a worker.
fn lookup_named(name: &str, size: u16) -> Option<PathBuf> {
    let found = icons::lookup(name, size).or_else(|| {
        (!name.ends_with("-symbolic"))
            .then(|| icons::lookup(&format!("{name}-symbolic"), size))
            .flatten()
    })?;
    if is_symbolic(&found)
        && let Ok(bytes) = std::fs::read(&found)
        && let Ok(mut preloaded) = preloaded().lock()
    {
        preloaded.insert(found.to_string_lossy().into_owned(), bytes.into());
    }
    Some(found)
}

impl AssetSource for FsAssets {
    fn load(&self, path: &str) -> anyhow::Result<Option<Cow<'static, [u8]>>> {
        if let Some(bytes) = crate::toolbar::asset(path) {
            return Ok(Some(Cow::Borrowed(bytes)));
        }
        if let Some(bytes) = preloaded().lock().ok().and_then(|p| p.get(path).cloned()) {
            return Ok(Some(Cow::Owned(bytes.to_vec())));
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
            // Or its symbolic variant, which `lookup_named` tries next.
            Want::Named(name) => match icons::try_cached(name, size) {
                Some(None) if !name.ends_with("-symbolic") => {
                    icons::try_cached(&format!("{name}-symbolic"), size)
                }
                other => other,
            },
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

    /// Icon path for an entry: the folder icon or the icon of the type it
    /// is shown as (`mime::of_entry`: a link's is what it leads to's).
    pub fn for_entry(&mut self, e: &Entry, size: u16) -> Option<Arc<Path>> {
        if e.is_dir() {
            return self.folder(size);
        }
        self.for_mime(mime::of_entry(e).as_ref(), size)
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
                            Want::Named(name) => lookup_named(name, size),
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

/// Render an icon path at `size` px, in full color (`svg()` would render
/// a monochrome mask). An SVG is rasterized for the side it is drawn at
/// (`SizedSvg`); any other picture is decoded by `img()` as it is.
pub fn element(path: &Path, size: f32) -> AnyElement {
    let source: ImageSource = if is_svg(path) {
        let path: Arc<Path> = path.into();
        ImageSource::Custom(Arc::new(move |window: &mut Window, cx: &mut gpui::App| {
            let pixels = raster_side(size * window.scale_factor());
            window.use_asset::<SizedSvg>(&(Arc::clone(&path), pixels), cx)
        }))
    } else {
        path.to_path_buf().into()
    };
    img(source).size(px(size)).flex_none().into_any_element()
}

/// Whether an icon file is an SVG drawing (`.svgz` compressed).
fn is_svg(path: &Path) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| e.eq_ignore_ascii_case("svg") || e.eq_ignore_ascii_case("svgz"))
}

/// The sides, in device pixels, an SVG icon is rasterized at: the drawn
/// side rounded up to one of these, so a zoom that moves the size a little
/// draws from the same bitmap, and at most the last, the largest any icon
/// was drawn from before (the grid's largest icon is enlarged past it).
const RASTER_SIDES: [u32; 9] = [16, 24, 32, 48, 64, 96, 128, 192, 256];

/// The side an icon drawn `device` pixels wide is rasterized at.
fn raster_side(device: f32) -> u32 {
    let device = device.ceil().max(1.) as u32;
    RASTER_SIDES
        .into_iter()
        .find(|side| *side >= device)
        .unwrap_or(RASTER_SIDES[RASTER_SIDES.len() - 1])
}

/// An SVG icon rasterized within a side of device pixels, loaded by GPUI's
/// asset system on its background executor and kept by it per (file,
/// side), as `img()` keeps a picture per file.
enum SizedSvg {}

impl Asset for SizedSvg {
    type Source = (Arc<Path>, u32);
    type Output = Result<Arc<RenderImage>, ImageCacheError>;

    // An `async fn` would hold `cx`, and GPUI wants a future that outlives
    // the call; the work stays in the future, on the background executor.
    #[allow(clippy::manual_async_fn)]
    fn load(
        (path, side): Self::Source,
        _cx: &mut gpui::App,
    ) -> impl Future<Output = Self::Output> + Send + 'static {
        async move {
            let drawn =
                icons::rasterize(&path, side).map_err(|e| ImageCacheError::Other(Arc::new(e)))?;
            crate::thumbs::owned_render_image(drawn.width, drawn.height, drawn.rgba).ok_or_else(
                || {
                    ImageCacheError::Other(Arc::new(anyhow::anyhow!(
                        "an icon's pixels do not fill its size"
                    )))
                },
            )
        }
    }
}

/// What an entry's picture says of a link: nothing, that it is one, or
/// that it leads nowhere.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Link {
    #[default]
    None,
    Leads,
    Broken,
}

impl Link {
    pub fn of(e: &Entry) -> Self {
        if !e.is_link() {
            Link::None
        } else if e.is_broken_link() {
            Link::Broken
        } else {
            Link::Leads
        }
    }
}

/// What an entry's picture is marked with on its corner: a link, leading
/// somewhere or nowhere, or a place that is a tag's, in its color (a place
/// is a folder holding files kept elsewhere). Every view marks entries
/// this one way (`marked`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mark {
    None,
    Link(Link),
    /// A tag's place, the tag's color.
    Tag(usize),
}

impl Mark {
    /// An entry's: the tag of a place that is a tag's, else its link.
    pub fn of(link: Link, tag: Option<usize>) -> Mark {
        match (tag, link) {
            (Some(color), _) => Mark::Tag(color),
            (None, Link::None) => Mark::None,
            (None, link) => Mark::Link(link),
        }
    }
}

/// `picture` (an icon or a thumbnail about `size` px) with `mark` on its
/// bottom-right corner, on a badge: a bent arrow for a link (in the error
/// color for one that leads nowhere), the tag's mark in its color for a
/// tag's place. Unmarked, it is returned as it is.
pub fn marked(
    picture: AnyElement,
    size: f32,
    mark: Mark,
    theme: &crate::theme::Theme,
) -> AnyElement {
    let (glyph, color) = match mark {
        Mark::None | Mark::Link(Link::None) => return picture,
        Mark::Link(Link::Leads) => ("link", theme.text),
        Mark::Link(Link::Broken) => ("link", theme.error),
        Mark::Tag(color) => ("tag", theme.tag(color)),
    };
    // Large enough to read on a list's 16 px icons, and growing slower
    // than the picture: a corner, never a cover.
    let badge = (size * 0.3 + 6.).round().clamp(11., 22.);
    div()
        .relative()
        .flex_none()
        .child(picture)
        .child(
            div()
                .absolute()
                .right(px(-1.))
                .bottom(px(-1.))
                .size(px(badge))
                .flex()
                .items_center()
                .justify_center()
                .rounded_sm()
                .bg(theme.bg)
                .border_1()
                .border_color(theme.border)
                .child(crate::controls::glyph(glyph, color, badge - 2.)),
        )
        .into_any_element()
}

/// An entry's picture at `size` px when it has no thumbnail: its theme
/// icon, else a square (a folder's in the accent) until the theme answers;
/// marked (`marked`). The one every view draws.
pub fn entry_picture(
    icon: Option<&Path>,
    is_dir: bool,
    size: f32,
    mark: Mark,
    theme: &crate::theme::Theme,
) -> AnyElement {
    let picture = match icon {
        Some(icon) => element(icon, size),
        None => div()
            .size(px(size))
            .flex_none()
            .rounded_sm()
            .bg(if is_dir {
                theme.accent
            } else {
                theme.text_muted
            })
            .into_any_element(),
    };
    marked(picture, size, mark, theme)
}

/// A named theme icon at `size` px: in full color, or tinted with `color`
/// when it is symbolic.
pub fn themed(path: &Path, size: f32, color: Hsla) -> AnyElement {
    if !is_symbolic(path) {
        return element(path, size);
    }
    svg()
        .path(SharedString::from(path.to_string_lossy().into_owned()))
        .size(px(size))
        .flex_none()
        .text_color(color)
        .into_any_element()
}

#[cfg(test)]
mod tests {
    use super::{is_svg, is_symbolic, raster_side};
    use std::path::Path;

    #[test]
    fn an_svg_icon_is_rasterized_at_a_side_at_least_as_large_as_drawn() {
        // A sidebar row's 16 px at scale 1 and 2, a list's, the grid's.
        assert_eq!(raster_side(16.), 16);
        assert_eq!(raster_side(32.), 32);
        assert_eq!(raster_side(16.5), 24);
        assert_eq!(raster_side(20. * 1.5), 32);
        assert_eq!(raster_side(33.), 48);
        assert_eq!(raster_side(0.), 16);
        // Never larger than an icon was drawn from before.
        assert_eq!(raster_side(512.), 256);
        assert!(is_svg(Path::new(
            "/icons/Adwaita/scalable/places/user-trash.svg"
        )));
        assert!(is_svg(Path::new("/icons/x/folder.SVGZ")));
        assert!(!is_svg(Path::new("/icons/hicolor/16x16/places/folder.png")));
    }

    #[test]
    fn a_symbolic_icon_is_known_by_its_name() {
        assert!(is_symbolic(Path::new(
            "/icons/Adwaita/symbolic/places/folder-saved-search-symbolic.svg"
        )));
        assert!(!is_symbolic(Path::new(
            "/icons/hicolor/16x16/places/folder.png"
        )));
        assert!(!is_symbolic(Path::new("/icons/x/symbolic-folder.svg")));
    }
}
