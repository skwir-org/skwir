//! Icon lookup through the platform crate, resolved on a worker and cached
//! per (name, size), and a GPUI `AssetSource` serving absolute filesystem paths so `svg()` works.
//!
//! A named icon the theme has only as a symbolic variant (current Adwaita
//! and Breeze ship `folder-saved-search` only as `-symbolic`) is used as
//! GTK does, drawn in the text color: a symbolic SVG is a shape to tint,
//! not a picture. So is an icon drawn in the color scheme's colors (KDE's
//! `current-color-scheme` stylesheet: Breeze's small icons), which its
//! theme expects recolored: drawn as its file says, it stays dark gray on
//! a dark theme. The worker that finds one reads it too (`shape_of`), so
//! drawing it never touches the disk.
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
use bibli_platform::system_icons::{self, SystemIcon};
use bibli_platform::{icons, mime};
use gpui::{
    AnyElement, Asset, AssetSource, Hsla, ImageCacheError, ImageSource, RenderImage, SharedString,
    Window, div, img, prelude::*, px, svg,
};

/// Serves any absolute path on disk. Icon themes live outside the binary.
pub struct FsAssets;

/// Shapes read by the lookup worker, by the path `svg()` asks for.
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
    icons::lookup(name, size).or_else(|| {
        (!name.ends_with("-symbolic"))
            .then(|| icons::lookup(&format!("{name}-symbolic"), size))
            .flatten()
    })
}

/// What `shape_of` found each theme icon file to be: a shape or not.
fn shapes() -> &'static Mutex<HashMap<PathBuf, bool>> {
    static SHAPES: std::sync::OnceLock<Mutex<HashMap<PathBuf, bool>>> = std::sync::OnceLock::new();
    SHAPES.get_or_init(Mutex::default)
}

/// An icon file larger than this is no shape: drawn as a picture.
const ICON_BYTES: u64 = 1 << 20;

/// Whether a theme's icon file is a shape to draw in one color: a
/// symbolic one (GTK's, by its name), or an SVG in the color scheme's
/// colors (KDE's `current-color-scheme` stylesheet). Reads the file the
/// first time, and keeps a shape's bytes for `svg()`: a worker's.
fn shape_of(path: &Path) -> bool {
    if let Some(known) = known_shape(path) {
        return known;
    }
    let bytes = (is_svg(path) && std::fs::metadata(path).is_ok_and(|m| m.len() <= ICON_BYTES))
        .then(|| std::fs::read(path).ok())
        .flatten();
    let shape = bytes.as_deref().is_some_and(|bytes| {
        is_symbolic(path)
            || bytes
                .windows(b"current-color-scheme".len())
                .any(|w| w == b"current-color-scheme")
    });
    if shape
        && let Some(bytes) = bytes
        && let Ok(mut preloaded) = preloaded().lock()
    {
        preloaded.insert(path.to_string_lossy().into_owned(), bytes.into());
    }
    if let Ok(mut shapes) = shapes().lock() {
        shapes.insert(path.to_path_buf(), shape);
    }
    shape
}

/// What `shape_of` found `path` to be, if it has read it. Never blocks.
fn known_shape(path: &Path) -> Option<bool> {
    shapes().lock().ok()?.get(path).copied()
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

/// What an icon is drawn from: a file of the desktop's icon theme, or the
/// system's own picture, where bibli reads no theme (macOS, Windows:
/// `bibli_platform::system_icons`).
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum Pic {
    /// A theme's icon, in its own colors.
    Theme(Arc<Path>),
    /// A theme's icon drawn as a shape in one color (`shape_of`).
    Shape(Arc<Path>),
    System(Arc<SystemIcon>),
}

impl Pic {
    /// A theme's icon file, as `shape_of` read it: blocks the first time.
    fn of_theme(path: PathBuf) -> Pic {
        if shape_of(&path) {
            Pic::Shape(path.into())
        } else {
            Pic::Theme(path.into())
        }
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
    /// What the system draws itself, where it does: a file's by its
    /// extension or its path.
    System(SystemIcon),
}

impl Want {
    /// A file known only by its name: its type's icon, or on a system
    /// that draws its own, its extension's.
    fn for_name(name: &std::ffi::OsStr) -> Want {
        if system_icons::AVAILABLE {
            Want::System(system_icons::by_extension(name))
        } else {
            Want::Mime(mime::by_name(name).map_or_else(String::new, |m| m.0))
        }
    }
}

/// What `want` is drawn from at `size`. Reads the icon theme: blocks, so a
/// worker's (`IconCache::request`, an inspection's rows).
fn resolve(want: &Want, size: u16) -> Option<Pic> {
    let system = |icon: SystemIcon| Some(Pic::System(Arc::new(icon)));
    let theme = |path: Option<PathBuf>| path.map(Pic::of_theme);
    match want {
        Want::System(icon) => system(icon.clone()),
        Want::Named(name) if system_icons::AVAILABLE => {
            system_icons::for_name(name).and_then(system)
        }
        Want::Folder if system_icons::AVAILABLE => system(SystemIcon::Folder),
        Want::Named(name) => theme(lookup_named(name, size)),
        Want::Folder => theme(icons::folder(size)),
        Want::Mime(m) if m.is_empty() => theme(icons::unknown(size)),
        Want::Mime(m) => theme(icons::for_mime(&mime::Mime(m.clone()), size)),
    }
}

/// A folder's icon, resolved where it may block (a worker).
pub fn folder_now(size: u16) -> Option<Pic> {
    resolve(&Want::Folder, size)
}

/// The icon of a file known only by its name, resolved where it may block
/// (a worker): an archive's member, an attachment.
pub fn for_name_now(name: &std::ffi::OsStr, size: u16) -> Option<Pic> {
    resolve(&Want::for_name(name), size)
}

type Resolved = Vec<((Want, u16), Option<Pic>)>;

/// How often a job of lookups hands over what it found so far.
const PUBLISH: std::time::Duration = std::time::Duration::from_millis(16);

/// Icons of the desktop's theme, or the system's, per size. Looking one up
/// may read the theme from disk, so a miss is answered `None` for now and
/// resolved on a worker with the others this frame missed (`request`);
/// `absorb` takes the answers and the next frame draws them. Drawing only
/// ever reads this map.
#[derive(Default)]
pub struct IconCache {
    /// Answers, per size then per name or type.
    known: HashMap<u16, HashMap<Want, Option<Pic>>>,
    wanted: HashSet<(Want, u16)>,
    results: Arc<Mutex<Resolved>>,
    /// The theme the setting names (`ui.icon_theme`); `None`: the
    /// desktop's.
    theme: Option<String>,
}

impl IconCache {
    /// Use the icon theme the setting names (`None`: the desktop's). What
    /// was looked up is forgotten and asked again, and answers still on
    /// their way, for the theme before, are dropped. Whether it changed.
    pub fn set_theme(&mut self, theme: Option<String>) -> bool {
        if self.theme == theme {
            return false;
        }
        icons::choose_theme(theme.as_deref());
        self.theme = theme;
        self.known.clear();
        self.wanted.clear();
        self.results = Arc::default();
        true
    }

    fn lookup(&mut self, want: Want, size: u16) -> Option<Pic> {
        if let Some(found) = self.known.get(&size).and_then(|known| known.get(&want)) {
            return found.clone();
        }
        // Resolved by an earlier worker, or by anyone else who asked.
        let cached = match &want {
            // Or its symbolic variant, which `lookup_named` tries next.
            Want::Named(name) if !system_icons::AVAILABLE => match icons::try_cached(name, size) {
                Some(None) if !name.ends_with("-symbolic") => {
                    icons::try_cached(&format!("{name}-symbolic"), size)
                }
                other => other,
            },
            _ => None,
        };
        // Whether a file is a shape is known once a worker read it.
        let found = match cached {
            Some(None) => Some(None),
            Some(Some(path)) => known_shape(&path).map(|shape| {
                Some(if shape {
                    Pic::Shape(path.into())
                } else {
                    Pic::Theme(path.into())
                })
            }),
            None => None,
        };
        let Some(found) = found else {
            self.wanted.insert((want, size));
            return None;
        };
        self.known
            .entry(size)
            .or_default()
            .insert(want, found.clone());
        found
    }

    /// An icon by the name the desktop's themes give it (`user-home`,
    /// `drive-harddisk`): the theme's, or the system's for it.
    pub fn named(&mut self, name: &str, size: u16) -> Option<Pic> {
        self.lookup(Want::Named(name.to_owned()), size)
    }

    /// The icon for an entry: the folder icon or the icon of the type it
    /// is shown as (`mime::of_entry`: a link's is what it leads to's); on
    /// a system that draws its own, its extension's, or its own (an
    /// application, a shortcut), read from the file at `path`, which is
    /// asked for only then.
    pub fn for_entry(
        &mut self,
        e: &Entry,
        path: impl FnOnce() -> Option<PathBuf>,
        size: u16,
    ) -> Option<Pic> {
        if system_icons::AVAILABLE {
            let icon = match path() {
                Some(path) => system_icons::for_file(&path, e.type_name(), e.is_dir()),
                None if e.is_dir() => SystemIcon::Folder,
                None => system_icons::by_extension(e.type_name()),
            };
            return self.lookup(Want::System(icon), size);
        }
        if e.is_dir() {
            return self.folder(size);
        }
        self.for_mime(mime::of_entry(e).as_ref(), size)
    }

    pub fn folder(&mut self, size: u16) -> Option<Pic> {
        self.lookup(Want::Folder, size)
    }

    /// The icon of a file by its name: its type's, the unknown-file icon
    /// when the name says nothing; its extension's on a system that draws
    /// its own.
    pub fn for_name(&mut self, name: &std::ffi::OsStr, size: u16) -> Option<Pic> {
        self.lookup(Want::for_name(name), size)
    }

    /// The icon of a MIME type already known; `None` is the unknown file.
    fn for_mime(&mut self, mime: Option<&mime::Mime>, size: u16) -> Option<Pic> {
        let key = mime.map_or_else(String::new, |m| m.0.clone());
        self.lookup(Want::Mime(key), size)
    }

    /// Resolve what this frame missed, in one job, handing over what it
    /// has every `PUBLISH` (a cold theme answers a frame's first types in
    /// a hundred milliseconds or more: each shows as it is found, not with
    /// the last). Call after drawing.
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
                let publish = |resolved: &mut Resolved| {
                    if let Ok(mut slot) = results.lock() {
                        slot.extend(resolved.drain(..));
                    }
                    if let Some(app) = weak.upgrade() {
                        app.wake();
                    }
                };
                let mut resolved: Resolved = Vec::new();
                let mut since = std::time::Instant::now();
                for (want, size) in wanted {
                    let pic = resolve(&want, size);
                    resolved.push(((want, size), pic));
                    if since.elapsed() >= PUBLISH {
                        publish(&mut resolved);
                        since = std::time::Instant::now();
                    }
                }
                publish(&mut resolved);
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
        let changed = found.iter().any(|(_, pic)| pic.is_some());
        for ((want, size), pic) in found {
            self.known.entry(size).or_default().insert(want, pic);
        }
        changed
    }
}

/// Render an icon at `size` px: a theme's file in its own colors, a shape
/// in `tint`, the system's picture.
pub fn element(pic: &Pic, size: f32, tint: Hsla) -> AnyElement {
    match pic {
        Pic::Theme(path) => file_element(path, size),
        Pic::Shape(path) => svg()
            .path(SharedString::from(path.to_string_lossy().into_owned()))
            .size(px(size))
            .flex_none()
            .text_color(tint)
            .into_any_element(),
        Pic::System(icon) => {
            let icon = Arc::clone(icon);
            img(ImageSource::Custom(Arc::new(
                move |window: &mut Window, cx: &mut gpui::App| {
                    let pixels = raster_side(size * window.scale_factor());
                    window.use_asset::<SystemPicture>(&(Arc::clone(&icon), pixels), cx)
                },
            )))
            .size(px(size))
            .flex_none()
            .into_any_element()
        }
    }
}

/// A theme's icon file at `size` px, in full color (`svg()` would render a
/// monochrome mask). An SVG is rasterized for the side it is drawn at
/// (`SizedSvg`); any other picture is decoded by `img()` as it is.
fn file_element(path: &Path, size: f32) -> AnyElement {
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

/// A system icon drawn within a side of device pixels
/// (`bibli_platform::system_icons::picture`), loaded by GPUI's asset
/// system on its background executor and kept per (icon, side).
enum SystemPicture {}

impl Asset for SystemPicture {
    type Source = (Arc<SystemIcon>, u32);
    type Output = Result<Arc<RenderImage>, ImageCacheError>;

    // As `SizedSvg::load`: the work stays in the future.
    #[allow(clippy::manual_async_fn)]
    fn load(
        (icon, side): Self::Source,
        _cx: &mut gpui::App,
    ) -> impl Future<Output = Self::Output> + Send + 'static {
        async move {
            let none =
                || ImageCacheError::Other(Arc::new(anyhow::anyhow!("the system has no such icon")));
            let picture = system_icons::picture(&icon, side)
                .map_err(|e| ImageCacheError::Other(Arc::new(e)))?
                .ok_or_else(none)?;
            crate::thumbs::to_render_image(&picture).ok_or_else(none)
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
    icon: Option<&Pic>,
    is_dir: bool,
    size: f32,
    mark: Mark,
    theme: &crate::theme::Theme,
) -> AnyElement {
    let picture = match icon {
        Some(icon) => element(icon, size, theme.text_muted),
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

/// A named icon at `size` px: in its own colors, or in `color` when it is
/// a shape (`Pic::Shape`).
pub fn themed(pic: &Pic, size: f32, color: Hsla) -> AnyElement {
    element(pic, size, color)
}

#[cfg(test)]
mod tests {
    use super::{IconCache, Pic, Want, is_svg, is_symbolic, preloaded, raster_side};
    use std::path::Path;
    use std::sync::Arc;

    #[test]
    fn another_theme_forgets_the_icons_and_what_was_on_its_way() {
        let mut cache = IconCache::default();
        let folder = Pic::Theme(Path::new("/icons/breeze/places/16/folder.svg").into());
        cache
            .known
            .entry(16)
            .or_default()
            .insert(Want::Folder, Some(folder.clone()));
        assert_eq!(cache.folder(16), Some(folder.clone()));
        let on_its_way = Arc::clone(&cache.results);
        assert!(cache.set_theme(Some("Adwaita".into())));
        assert!(
            !cache.set_theme(Some("Adwaita".into())),
            "the same theme again"
        );
        // An answer for the theme before arrives: it is not taken.
        on_its_way
            .lock()
            .unwrap()
            .push(((Want::Folder, 16), Some(folder)));
        assert!(!cache.absorb());
        assert_eq!(cache.folder(16), None, "asked again");
        cache.set_theme(None);
    }

    #[test]
    fn an_icon_in_the_color_schemes_colors_is_a_shape_drawn_in_the_text_color() {
        let dir = tempfile::tempdir().unwrap();
        // Breeze's small icons: a stylesheet whose colors KDE replaces.
        let recolored = dir.path().join("user-home.svg");
        std::fs::write(
            &recolored,
            r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 16 16"><defs><style type="text/css" id="current-color-scheme">.ColorScheme-Text { color:#232629; }</style></defs><path style="fill:currentColor" d="M2 2h12v12H2z" class="ColorScheme-Text"/></svg>"#,
        )
        .unwrap();
        let picture = dir.path().join("folder.svg");
        std::fs::write(
            &picture,
            r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 16 16"><path fill="#3daee9" d="M2 2h12v12H2z"/></svg>"##,
        )
        .unwrap();
        let symbolic = dir.path().join("folder-saved-search-symbolic.svg");
        std::fs::write(&symbolic, r#"<svg xmlns="http://www.w3.org/2000/svg"/>"#).unwrap();
        let png = dir.path().join("folder.png");
        std::fs::write(&png, b"not read").unwrap();

        assert_eq!(
            Pic::of_theme(recolored.clone()),
            Pic::Shape(recolored.clone().into())
        );
        assert_eq!(Pic::of_theme(picture.clone()), Pic::Theme(picture.into()));
        assert_eq!(
            Pic::of_theme(symbolic.clone()),
            Pic::Shape(symbolic.clone().into())
        );
        assert_eq!(Pic::of_theme(png.clone()), Pic::Theme(png.into()));
        // A shape's bytes are kept for `svg()`, which draws it.
        let kept = preloaded().lock().unwrap();
        assert!(kept.contains_key(&*recolored.to_string_lossy()));
        assert!(kept.contains_key(&*symbolic.to_string_lossy()));
    }

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
