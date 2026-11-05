//! Icon lookup through the platform crate, resolved on a worker and cached
//! per (name, size), and a GPUI `AssetSource` serving absolute filesystem paths so `svg()` works.
//!
//! A theme's icon is one of three kinds (`Kind`), which the worker that
//! finds it reads (`kind_of`):
//!
//! - An icon drawn in KDE's color scheme (its `current-color-scheme`
//!   stylesheet: most of Breeze) names its colors by their part, text,
//!   accent, a state, and leaves them for whoever draws it to fill, as
//!   KDE's icon loader fills them from the palette. Skwir fills them from
//!   the theme (`Ink`) and draws it in full color: drawn as its file says,
//!   it stays dark gray on a dark theme, and drawn in one color, a large
//!   folder's layers (its body in the accent, a flap and a shadow over it)
//!   run into one shape in the text's color.
//! - A symbolic icon (GTK's, by its `-symbolic` name; current Adwaita and
//!   Breeze ship `folder-saved-search` only so) is a shape drawn in the
//!   text's color, as GTK draws it. A named icon the theme has only as one
//!   is used so. The worker keeps its bytes, so drawing it never touches
//!   the disk.
//! - Any other is a picture in its own colors.
//!
//! A full-color SVG icon is rasterized at the size it is drawn, in device
//! pixels rounded up to a few sides (`raster_side`), never at the size its
//! file declares: Adwaita's are 128 px, which `img()` drew at 256 by 256
//! for a 16 px row, filters and all.
//!
//! Where the system draws its own icons (macOS, Windows), a file may have
//! an icon of its own (an application, a shortcut): one picture per file,
//! which GPUI's asset cache and atlas would keep for good. The most drawn
//! lately are kept (`OwnIcons`); the window gives back the others' when a
//! frame begins (`let_go`).

use std::borrow::Cow;
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, Weak};

use gpui::{
    AnyElement, Asset, AssetSource, Hsla, ImageCacheError, ImageSource, RenderImage, SharedString,
    Window, div, img, prelude::*, px, svg,
};
use skwir_core::thumbs::Image;
use skwir_core::{App, Entry};
use skwir_platform::system_icons::{self, SystemIcon};
use skwir_platform::{icons, mime};

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

/// What a theme's icon file is, and so how it is drawn.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Kind {
    /// In its own colors.
    Picture,
    /// In KDE's color scheme, filled from the theme (`Ink`).
    Scheme,
    /// One color: a symbolic icon.
    Shape,
}

/// What `kind_of` found each theme icon file to be.
fn kinds() -> &'static Mutex<HashMap<PathBuf, Kind>> {
    static KINDS: std::sync::OnceLock<Mutex<HashMap<PathBuf, Kind>>> = std::sync::OnceLock::new();
    KINDS.get_or_init(Mutex::default)
}

/// An icon file larger than this is read no further: drawn as a picture.
const ICON_BYTES: u64 = 1 << 20;

/// What a theme's icon file is (`Kind`): an SVG in KDE's color scheme (its
/// `current-color-scheme` stylesheet), a symbolic one (GTK's, by its name),
/// or a picture. Reads the file the first time, and keeps a shape's bytes
/// for `svg()`: a worker's.
fn kind_of(path: &Path) -> Kind {
    if let Some(known) = known_kind(path) {
        return known;
    }
    let bytes = (is_svg(path) && std::fs::metadata(path).is_ok_and(|m| m.len() <= ICON_BYTES))
        .then(|| std::fs::read(path).ok())
        .flatten();
    let kind = match bytes.as_deref() {
        Some(bytes)
            if bytes
                .windows(b"current-color-scheme".len())
                .any(|w| w == b"current-color-scheme") =>
        {
            Kind::Scheme
        }
        Some(_) if is_symbolic(path) => Kind::Shape,
        _ => Kind::Picture,
    };
    if kind == Kind::Shape
        && let Some(bytes) = bytes
        && let Ok(mut preloaded) = preloaded().lock()
    {
        preloaded.insert(path.to_string_lossy().into_owned(), bytes.into());
    }
    if let Ok(mut kinds) = kinds().lock() {
        kinds.insert(path.to_path_buf(), kind);
    }
    kind
}

/// What `kind_of` found `path` to be, if it has read it. Never blocks.
fn known_kind(path: &Path) -> Option<Kind> {
    kinds().lock().ok()?.get(path).copied()
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
/// system's own picture, where skwir reads no theme (macOS, Windows:
/// `skwir_platform::system_icons`).
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum Pic {
    /// A theme's icon, in its own colors.
    Theme(Arc<Path>),
    /// A theme's icon in KDE's color scheme, filled from the theme
    /// (`Kind::Scheme`).
    Scheme(Arc<Path>),
    /// A theme's icon drawn as a shape in one color (`Kind::Shape`).
    Shape(Arc<Path>),
    System(Arc<SystemIcon>),
}

impl Pic {
    /// A theme's icon file, as `kind_of` read it: blocks the first time.
    fn of_theme(path: PathBuf) -> Pic {
        let kind = kind_of(&path);
        Pic::of_kind(path, kind)
    }

    fn of_kind(path: PathBuf, kind: Kind) -> Pic {
        match kind {
            Kind::Picture => Pic::Theme(path.into()),
            Kind::Scheme => Pic::Scheme(path.into()),
            Kind::Shape => Pic::Shape(path.into()),
        }
    }
}

/// The colors an icon is drawn in: a shape's (`Pic::Shape`) its text's,
/// and what an icon in KDE's color scheme (`Pic::Scheme`) names, filled as
/// KDE's icon loader fills them from the palette: its text (and buttons'
/// text) in the color the caller draws text in there, its background the
/// ground it is drawn on, its accent (and highlight) and states (positive,
/// neutral, negative) the theme's. Straight RGBA, so it keys what is drawn
/// in it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Ink {
    text: [u8; 4],
    ground: [u8; 4],
    accent: [u8; 4],
    positive: [u8; 4],
    neutral: [u8; 4],
    negative: [u8; 4],
}

impl Ink {
    /// Text in `text`, on the theme's ground.
    pub fn of(theme: &crate::theme::Theme, text: Hsla) -> Ink {
        Ink {
            text: rgba_bytes(text),
            ground: rgba_bytes(theme.bg),
            accent: rgba_bytes(theme.accent),
            positive: rgba_bytes(theme.success),
            neutral: rgba_bytes(theme.warning),
            negative: rgba_bytes(theme.error),
        }
    }

    /// Drawn on `ground` (a chosen row's selection) instead.
    #[must_use]
    pub fn on(self, ground: Hsla) -> Ink {
        Ink {
            ground: rgba_bytes(ground),
            ..self
        }
    }

    /// What a shape is drawn in: the text's color.
    fn tint(&self) -> Hsla {
        let [r, g, b, a] = self.text.map(|c| f32::from(c) / 255.);
        gpui::Rgba { r, g, b, a }.into()
    }

    /// The stylesheet that fills KDE's color scheme with these colors, laid
    /// under the icon's own and winning over it (`!important`).
    fn style_sheet(&self) -> String {
        let css = |[r, g, b, a]: [u8; 4]| format!("rgba({r},{g},{b},{})", f32::from(a) / 255.);
        let rule = |classes: &[&str], color: [u8; 4]| {
            let selector: Vec<String> = classes
                .iter()
                .map(|c| format!(".ColorScheme-{c}"))
                .collect();
            format!("{}{{color:{} !important}}", selector.join(","), css(color))
        };
        [
            rule(&["Text", "ButtonText", "ViewText"], self.text),
            rule(
                &["Background", "ViewBackground", "ButtonBackground"],
                self.ground,
            ),
            rule(&["Accent", "Highlight", "ActiveText"], self.accent),
            rule(&["PositiveText"], self.positive),
            rule(&["NeutralText"], self.neutral),
            rule(&["NegativeText"], self.negative),
        ]
        .concat()
    }
}

/// A color as straight RGBA bytes.
fn rgba_bytes(color: Hsla) -> [u8; 4] {
    let rgba = color.to_rgb();
    let byte = |c: f32| (c.clamp(0., 1.) * 255.).round() as u8;
    [byte(rgba.r), byte(rgba.g), byte(rgba.b), byte(rgba.a)]
}

/// What the inks icons are drawn in read of a theme (`Ink::of`,
/// `Ink::on`): its accent and states, the grounds icons are drawn on (the
/// window's, a chosen row's) and the colors text is drawn in beside them.
/// Only these moving redraws what is drawn in KDE's color scheme
/// (`colors_now`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct InkColors {
    accent: [u8; 4],
    positive: [u8; 4],
    neutral: [u8; 4],
    negative: [u8; 4],
    grounds: [[u8; 4]; 2],
    texts: [[u8; 4]; 3],
}

impl InkColors {
    pub fn of(theme: &crate::theme::Theme) -> Self {
        InkColors {
            accent: rgba_bytes(theme.accent),
            positive: rgba_bytes(theme.success),
            neutral: rgba_bytes(theme.warning),
            negative: rgba_bytes(theme.error),
            grounds: [theme.bg, theme.selection_bg].map(rgba_bytes),
            texts: [theme.text, theme.text_muted, theme.selection_text].map(rgba_bytes),
        }
    }
}

impl Ink {
    /// Whether it is one of `colors`' inks: a theme's that draws in them.
    fn is_of(&self, colors: &InkColors) -> bool {
        self.accent == colors.accent
            && self.positive == colors.positive
            && self.neutral == colors.neutral
            && self.negative == colors.negative
            && colors.grounds.contains(&self.ground)
            && colors.texts.contains(&self.text)
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
    /// What an entry's icon is looked up by: the folder or the type it is
    /// shown as (`mime::of_entry`: a link's is what it leads to's); on a
    /// system that draws its own, its extension's, or its own (an
    /// application, a shortcut), read from the file at `path`, which is
    /// asked for only then.
    fn of_entry(e: &Entry, path: impl FnOnce() -> Option<PathBuf>) -> Want {
        if system_icons::AVAILABLE {
            return Want::System(match path() {
                Some(path) => system_icons::for_file(&path, e.type_name(), e.is_dir()),
                None if e.is_dir() => SystemIcon::Folder,
                None => system_icons::by_extension(e.type_name()),
            });
        }
        if e.is_dir() {
            return Want::Folder;
        }
        Want::Mime(mime::of_entry(e).map_or_else(String::new, |m| m.0))
    }

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
    /// Files' own icons, the most drawn lately.
    own: OwnIcons,
}

/// How many files' own icons are kept beyond what the last two frames
/// drew: a folder of applications scrolled through keeps no more.
const OWN_KEPT: usize = 512;

/// The files whose own icons were drawn (`SystemIcon::Path`), the most
/// recent kept: what this frame and the one before drew never let go of,
/// the others once more than `OWN_KEPT` are (`ThumbCache` keeps its
/// pictures so). What is let go waits for the next frame to begin.
struct OwnIcons {
    lru: skwir_core::lru::ByteLru<SystemIcon, SystemIcon>,
    retired: Vec<SystemIcon>,
    /// The recency clock when this frame and the one before began.
    frame: u64,
    before: u64,
}

impl Default for OwnIcons {
    fn default() -> Self {
        OwnIcons {
            // Each counts one: the budget is a count of files.
            lru: skwir_core::lru::ByteLru::new(OWN_KEPT),
            retired: Vec::new(),
            frame: 0,
            before: 0,
        }
    }
}

impl OwnIcons {
    /// `icon` drawn now. What that lets go of is returned, and waits for
    /// the next frame (`begin_frame`).
    fn drawn(&mut self, icon: &SystemIcon) -> Vec<SystemIcon> {
        if self.lru.get(icon).is_some() {
            return Vec::new();
        }
        let gone = self
            .lru
            .insert_keeping(icon.clone(), icon.clone(), 1, self.before.max(1));
        self.retired.extend(gone.iter().cloned());
        gone
    }

    /// A frame begins: what was let go since, and from now on what this
    /// frame draws is kept too.
    fn begin_frame(&mut self) -> Vec<SystemIcon> {
        self.before = self.frame;
        self.frame = self.lru.now() + 1;
        std::mem::take(&mut self.retired)
    }
}

/// The pictures drawn of files' own icons (`SystemPicture`), by icon and
/// side, as long as GPUI keeps them: what `let_go` gives back to the atlas.
fn own_drawn() -> &'static Mutex<HashMap<(SystemIcon, u32), Weak<RenderImage>>> {
    static DRAWN: std::sync::OnceLock<Mutex<HashMap<(SystemIcon, u32), Weak<RenderImage>>>> =
        std::sync::OnceLock::new();
    DRAWN.get_or_init(Mutex::default)
}

/// Give back what the window drew of files' own icons that `icons` lets go
/// of (`IconCache::begin_frame`): GPUI's asset cache keeps a picture until
/// told to forget it, and the atlas its texture until it is dropped. At
/// every side it was drawn at.
pub fn let_go(icons: Vec<SystemIcon>, window: &mut Window, cx: &mut gpui::App) {
    if icons.is_empty() {
        return;
    }
    let drawn: Vec<(SystemIcon, u32, Option<Arc<RenderImage>>)> = {
        let Ok(mut kept) = own_drawn().lock() else {
            return;
        };
        icons
            .into_iter()
            .flat_map(|icon| RASTER_SIDES.map(|side| (icon.clone(), side)))
            .filter_map(|key| {
                let image = kept.remove(&key)?;
                Some((key.0, key.1, image.upgrade()))
            })
            .collect()
    };
    for (icon, side, image) in drawn {
        cx.remove_asset::<SystemPicture>(&(Arc::new(icon), side));
        if let Some(image) = image {
            cx.drop_image(image, Some(&mut *window));
        }
    }
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

    /// A frame begins: the files' own icons let go of since, for the
    /// window to give back (`let_go`).
    pub fn begin_frame(&mut self) -> Vec<SystemIcon> {
        self.own.begin_frame()
    }

    fn lookup(&mut self, want: Want, size: u16) -> Option<Pic> {
        // A file's own icon is kept while drawn lately; one let go is
        // forgotten here too, and asked again if drawn again.
        if let Want::System(icon @ SystemIcon::Path(_)) = &want {
            for gone in self.own.drawn(icon) {
                let gone = Want::System(gone);
                for known in self.known.values_mut() {
                    known.remove(&gone);
                }
                self.wanted.retain(|(want, _)| *want != gone);
            }
        }
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
        // What a file is, is known once a worker read it.
        let found = match cached {
            Some(None) => Some(None),
            Some(Some(path)) => known_kind(&path).map(|kind| Some(Pic::of_kind(path, kind))),
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

    /// The icon for an entry (`Want::of_entry`), its file at `path` asked
    /// for only where the system draws its own.
    pub fn for_entry(
        &mut self,
        e: &Entry,
        path: impl FnOnce() -> Option<PathBuf>,
        size: u16,
    ) -> Option<Pic> {
        self.lookup(Want::of_entry(e, path), size)
    }

    /// The icon `for_entry` gives, as pixels `side` wide, in `ink`: what
    /// `pixels` draws, for a picture that leaves the window.
    pub fn pixels_for_entry(
        &self,
        e: &Entry,
        path: impl FnOnce() -> Option<PathBuf>,
        side: u32,
        ink: Ink,
    ) -> IconPixels {
        IconPixels {
            want: Want::of_entry(e, path),
            side,
            ink,
            theme: self.theme.clone(),
        }
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
            skwir_core::scheduler::Priority::Visible,
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

/// Render an icon at `size` px: a theme's file in its own colors or in
/// `ink` (KDE's color scheme filled, a shape in its text's color), the
/// system's picture.
pub fn element(pic: &Pic, size: f32, ink: Ink) -> AnyElement {
    match pic {
        Pic::Theme(path) => file_element(path, size),
        Pic::Scheme(path) => {
            let path = Arc::clone(path);
            img(ImageSource::Custom(Arc::new(
                move |window: &mut Window, cx: &mut gpui::App| {
                    let pixels = raster_side(size * window.scale_factor());
                    match window.use_asset::<SchemeSvg>(&(Arc::clone(&path), pixels, ink), cx) {
                        Some(Ok(image)) => {
                            // Drawn in the colors now: what was drawn there
                            // before it, given back, leaves the atlas.
                            if RETIRING.load(std::sync::atomic::Ordering::Acquire) {
                                let place = (Arc::clone(&path), pixels);
                                let retired = scheme_drawn().replaced(&place, &image);
                                if let Some(retired) = retired {
                                    cx.drop_image(retired, Some(&mut *window));
                                }
                            }
                            Some(Ok(image))
                        }
                        // While it is rasterized in this ink, as it was
                        // drawn last: in other colors, never blank.
                        None => drawn_last(&path, pixels).map(Ok),
                        failed => failed,
                    }
                },
            )))
            .size(px(size))
            .flex_none()
            .into_any_element()
        }
        Pic::Shape(path) => svg()
            .path(SharedString::from(path.to_string_lossy().into_owned()))
            .size(px(size))
            .flex_none()
            .text_color(ink.tint())
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

/// skwir's own icon, the one its packages install
/// (`assets/org.skwir.skwir.svg`).
pub(crate) const APP_ICON: &[u8] = include_bytes!("../assets/org.skwir.skwir.svg");

/// skwir's icon at `size` px, rasterized for the side it is drawn at, as
/// a theme's SVG is.
pub fn app_icon(size: f32) -> AnyElement {
    img(ImageSource::Custom(Arc::new(
        move |window: &mut Window, cx: &mut gpui::App| {
            let pixels = raster_side(size * window.scale_factor());
            window.use_asset::<AppIcon>(&pixels, cx)
        },
    )))
    .size(px(size))
    .flex_none()
    .into_any_element()
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
        async move { render_image(icons::rasterize(&path, side)) }
    }
}

/// An SVG icon in KDE's color scheme (`Pic::Scheme`) rasterized within a
/// side of device pixels in an `Ink`, kept per (file, side, ink) as
/// `SizedSvg` keeps a file's. What other colors drew is given back once
/// the theme's are settled (`colors_now`).
enum SchemeSvg {}

type SchemeKey = (Arc<Path>, u32, Ink);

/// A file at a side of device pixels: where `SchemeDrawn` keeps what it
/// was drawn as last.
type Place = (Arc<Path>, u32);

/// What was drawn of icons in KDE's color scheme.
#[derive(Default)]
struct SchemeDrawn {
    /// Each raster, as long as GPUI keeps it: what `colors_now` gives back.
    drawn: HashMap<SchemeKey, Weak<RenderImage>>,
    /// The last rasterized of each file at each side, drawn while it is
    /// rasterized in other colors (`element`): at most one a file and
    /// side, the one GPUI keeps for the colors now once they are drawn.
    last: HashMap<Place, Arc<RenderImage>>,
    /// Rasters given back while still the last of their place, waiting
    /// for it in the colors now: dropped from the atlas once that is drawn
    /// (`element`), else at the next settling (`settle`).
    retiring: HashMap<Place, Arc<RenderImage>>,
    /// The colors given back to last (`colors_now`), and whether a value
    /// was shown before it was taken since.
    settled: Option<InkColors>,
    previewed: bool,
}

/// What a settling gives back: the rasters GPUI's asset cache forgets,
/// and those the atlas drops now.
#[derive(Default)]
struct GivenBack {
    forgotten: Vec<SchemeKey>,
    dropped: Vec<Arc<RenderImage>>,
}

/// Whether `SchemeDrawn::retiring` holds any: what `element` asks each
/// time it draws, without the lock.
static RETIRING: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

impl SchemeDrawn {
    /// `image` rasterized for `key`: kept, and its place's last.
    fn loaded(&mut self, key: SchemeKey, image: &Arc<RenderImage>) {
        self.last
            .insert((Arc::clone(&key.0), key.1), Arc::clone(image));
        self.drawn.insert(key, Arc::downgrade(image));
    }

    /// The colors are `now`'s: every raster in other colors is given back.
    /// One that is the last of its place gives way there to its place's
    /// raster in the colors now, else stays its last, drawn until there
    /// is one (`retiring`); no raster given back is drawn from `last`
    /// after the atlas let go of it.
    fn settle(&mut self, now: &InkColors) -> GivenBack {
        let current: HashMap<Place, Arc<RenderImage>> = self
            .drawn
            .iter()
            .filter(|((_, _, ink), _)| ink.is_of(now))
            .filter_map(|((path, side, _), image)| {
                Some(((Arc::clone(path), *side), image.upgrade()?))
            })
            .collect();
        let mut given = GivenBack::default();
        // What waited from the settling before goes now: colors moved
        // twice while its place was not drawn again.
        for (place, image) in self.retiring.drain() {
            if self
                .last
                .get(&place)
                .is_some_and(|last| Arc::ptr_eq(last, &image))
            {
                match current.get(&place) {
                    Some(now) => self.last.insert(place, Arc::clone(now)),
                    None => self.last.remove(&place),
                };
            }
            given.dropped.push(image);
        }
        let gone: Vec<SchemeKey> = self
            .drawn
            .keys()
            .filter(|(_, _, ink)| !ink.is_of(now))
            .cloned()
            .collect();
        for key in gone {
            let image = self.drawn.remove(&key).and_then(|weak| weak.upgrade());
            let place = (Arc::clone(&key.0), key.1);
            given.forgotten.push(key);
            let Some(image) = image else { continue };
            if self
                .last
                .get(&place)
                .is_some_and(|last| Arc::ptr_eq(last, &image))
            {
                if let Some(now) = current.get(&place) {
                    self.last.insert(place, Arc::clone(now));
                } else {
                    self.retiring.insert(place, image);
                    continue;
                }
            }
            given.dropped.push(image);
        }
        RETIRING.store(
            !self.retiring.is_empty(),
            std::sync::atomic::Ordering::Release,
        );
        given
    }

    /// `place` is drawn in the colors now, as `image`: what retired there
    /// goes, and `image` is its last.
    fn replaced(&mut self, place: &Place, image: &Arc<RenderImage>) -> Option<Arc<RenderImage>> {
        let retired = self.retiring.remove(place)?;
        if self
            .last
            .get(place)
            .is_some_and(|last| Arc::ptr_eq(last, &retired))
        {
            self.last.insert(place.clone(), Arc::clone(image));
        }
        RETIRING.store(
            !self.retiring.is_empty(),
            std::sync::atomic::Ordering::Release,
        );
        Some(retired)
    }
}

fn scheme_drawn() -> std::sync::MutexGuard<'static, SchemeDrawn> {
    static DRAWN: std::sync::OnceLock<Mutex<SchemeDrawn>> = std::sync::OnceLock::new();
    DRAWN
        .get_or_init(Mutex::default)
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

/// What `path` was rasterized in last at `side`, in whatever colors.
fn drawn_last(path: &Arc<Path>, side: u32) -> Option<Arc<RenderImage>> {
    scheme_drawn().last.get(&(Arc::clone(path), side)).cloned()
}

impl Asset for SchemeSvg {
    type Source = SchemeKey;
    type Output = Result<Arc<RenderImage>, ImageCacheError>;

    // As `SizedSvg::load`: the work stays in the future.
    #[allow(clippy::manual_async_fn)]
    fn load(
        (path, side, ink): Self::Source,
        _cx: &mut gpui::App,
    ) -> impl Future<Output = Self::Output> + Send + 'static {
        async move {
            let sheet = ink.style_sheet();
            let image = render_image(icons::rasterize_styled(&path, side, Some(&sheet)))?;
            scheme_drawn().loaded((path, side, ink), &image);
            Ok(image)
        }
    }
}

/// The theme's colors are `theme`'s, or `previewing`: a value shown
/// before it is taken (Colors from's list, `settings::previewed`). Once
/// they are settled, what icons in KDE's color scheme were drawn in other
/// colors is given back (GPUI's asset cache keeps a picture until told to
/// forget it, the atlas its texture until it is dropped): while values
/// are shown, every one's is kept, so going back to one draws it at once.
/// Only the colors an ink reads count (`InkColors`). A file drawn in new
/// colors shows as it was last drawn until it is rasterized in them.
pub fn colors_now(
    theme: &crate::theme::Theme,
    previewing: bool,
    window: &mut Window,
    cx: &mut gpui::App,
) {
    let given = {
        let mut drawn = scheme_drawn();
        if previewing {
            drawn.previewed = true;
            return;
        }
        let now = InkColors::of(theme);
        if !drawn.previewed && drawn.settled == Some(now) {
            return;
        }
        drawn.settled = Some(now);
        drawn.previewed = false;
        drawn.settle(&now)
    };
    for key in given.forgotten {
        cx.remove_asset::<SchemeSvg>(&key);
    }
    for image in given.dropped {
        cx.drop_image(image, Some(&mut *window));
    }
}

/// skwir's icon (`APP_ICON`) rasterized within a side of device pixels,
/// kept per side as `SizedSvg` keeps a file's.
enum AppIcon {}

impl Asset for AppIcon {
    type Source = u32;
    type Output = Result<Arc<RenderImage>, ImageCacheError>;

    // As `SizedSvg::load`: the work stays in the future.
    #[allow(clippy::manual_async_fn)]
    fn load(side: u32, _cx: &mut gpui::App) -> impl Future<Output = Self::Output> + Send + 'static {
        async move { render_image(skwir_platform::inspection::drawing_within(APP_ICON, side)) }
    }
}

/// A rasterized SVG as GPUI draws it.
fn render_image(
    drawn: anyhow::Result<skwir_platform::inspection::Decoded>,
) -> Result<Arc<RenderImage>, ImageCacheError> {
    let drawn = drawn.map_err(|e| ImageCacheError::Other(Arc::new(e)))?;
    crate::thumbs::owned_render_image(drawn.width, drawn.height, drawn.rgba).ok_or_else(|| {
        ImageCacheError::Other(Arc::new(anyhow::anyhow!(
            "an icon's pixels do not fill its size"
        )))
    })
}

/// A system icon drawn within a side of device pixels
/// (`skwir_platform::system_icons::picture`), loaded by GPUI's asset
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
            let image = crate::thumbs::to_render_image(&picture).ok_or_else(none)?;
            // A file's own, one per file: what the window gives back once
            // it is no longer drawn (`let_go`).
            if matches!(*icon, SystemIcon::Path(_))
                && let Ok(mut drawn) = own_drawn().lock()
            {
                drawn.insert(((*icon).clone(), side), Arc::downgrade(&image));
            }
            Ok(image)
        }
    }
}

/// An entry's icon as straight RGBA pixels, for a picture that leaves the
/// window and must be pixels of its own (a drag's, which the compositor
/// shows once it leaves): what it is looked up by, the side it is drawn
/// within, the colors it is drawn in, and the theme it was asked of.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct IconPixels {
    want: Want,
    side: u32,
    ink: Ink,
    theme: Option<String>,
}

impl IconPixels {
    /// Looked up and drawn: blocks on the theme and the icon's file.
    fn draw(&self) -> Option<Image> {
        let decoded = |drawn: skwir_platform::inspection::Decoded| Image {
            width: drawn.width,
            height: drawn.height,
            rgba: drawn.rgba.into(),
        };
        match resolve(&self.want, u16::try_from(self.side).unwrap_or(u16::MAX))? {
            Pic::Theme(path) => icon_file(&path, self.side).map(decoded),
            Pic::Scheme(path) => {
                icons::rasterize_styled(&path, self.side, Some(&self.ink.style_sheet()))
                    .ok()
                    .map(decoded)
            }
            Pic::Shape(path) => icon_file(&path, self.side).map(|mut drawn| {
                tint(&mut drawn.rgba, self.ink.text);
                decoded(drawn)
            }),
            Pic::System(icon) => system_icons::picture(&icon, self.side).ok()?,
        }
    }
}

/// A shape's pixels (straight RGBA) in `color`, as `svg()` draws one: its
/// coverage, in one color.
fn tint(rgba: &mut [u8], color: [u8; 4]) {
    let [r, g, b, a] = color;
    for p in rgba.as_chunks_mut::<4>().0 {
        *p = [r, g, b, (u16::from(p[3]) * u16::from(a) / 255) as u8];
    }
}

/// A theme's icon file drawn within `side` pixels: an SVG fitted to it, a
/// picture shrunk to it.
fn icon_file(path: &Path, side: u32) -> Option<skwir_platform::inspection::Decoded> {
    use skwir_platform::inspection;
    if is_svg(path) {
        return icons::rasterize(path, side).ok();
    }
    let cancel = skwir_core::CancelToken::new();
    let bytes =
        inspection::read_file(path, inspection::limits::Bounds::QUICK, "Icon", &cancel).ok()?;
    inspection::picture(&bytes, side).ok()
}

/// `icon`'s pixels once looked up and drawn on GPUI's background executor,
/// and kept by its asset cache; until then `None`, and the view drawing
/// is drawn again when they arrive. `Some(None)`: no icon to draw.
pub fn pixels(icon: &IconPixels, window: &mut Window, cx: &mut gpui::App) -> Option<Option<Image>> {
    window.use_asset::<EntryPixels>(icon, cx)
}

enum EntryPixels {}

impl Asset for EntryPixels {
    type Source = IconPixels;
    type Output = Option<Image>;

    // As `SizedSvg::load`: the work stays in the future.
    #[allow(clippy::manual_async_fn)]
    fn load(
        icon: IconPixels,
        _cx: &mut gpui::App,
    ) -> impl Future<Output = Self::Output> + Send + 'static {
        async move { icon.draw() }
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
        Some(icon) => element(icon, size, Ink::of(theme, theme.text_muted)),
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

/// A named icon at `size` px: in its own colors, or in `ink` (`element`).
pub fn themed(pic: &Pic, size: f32, ink: Ink) -> AnyElement {
    element(pic, size, ink)
}

#[cfg(test)]
mod tests {
    use super::{
        APP_ICON, IconCache, Ink, OWN_KEPT, OwnIcons, Pic, SystemIcon, Want, is_svg, is_symbolic,
        preloaded, raster_side, tint,
    };
    use std::path::{Path, PathBuf};
    use std::sync::Arc;

    #[test]
    fn skwirs_icon_is_drawn_at_the_side_asked() {
        let drawn = skwir_platform::inspection::drawing_within(APP_ICON, 64).unwrap();
        assert_eq!((drawn.width, drawn.height), (64, 64));
        let pixel = |x: usize, y: usize| &drawn.rgba[(y * 64 + x) * 4..][..4];
        assert_eq!(pixel(0, 0)[3], 0, "a rounded corner shows what is behind");
        assert_eq!(pixel(32, 3), [0x20, 0x30, 0x42, 0xff], "the ink tile");
    }

    #[test]
    fn a_shape_drawn_for_a_drag_takes_its_tint_by_its_coverage() {
        let mut rgba = vec![46, 52, 54, 255, 46, 52, 54, 128, 0, 0, 0, 0];
        tint(&mut rgba, [200, 210, 220, 255]);
        assert_eq!(
            rgba,
            [200, 210, 220, 255, 200, 210, 220, 128, 200, 210, 220, 0]
        );
        let mut half = vec![0, 0, 0, 255];
        tint(&mut half, [10, 20, 30, 128]);
        assert_eq!(half, [10, 20, 30, 128]);
    }

    #[test]
    fn a_file_drawn_lately_keeps_its_own_icon() {
        let icon = |n: usize| SystemIcon::Path(PathBuf::from(format!("/apps/app {n}.exe")));
        let mut own = OwnIcons::default();
        own.begin_frame();
        // More than are kept, all in one frame: none let go while drawn.
        for n in 0..OWN_KEPT + 10 {
            assert!(own.drawn(&icon(n)).is_empty());
        }
        assert!(own.begin_frame().is_empty(), "drawn this frame");
        // Drawn again in the next frame, the first ten only: the oldest of
        // the rest go once this frame and the one before no longer hold
        // them.
        for n in 0..10 {
            own.drawn(&icon(n));
        }
        own.begin_frame();
        let mut gone = Vec::new();
        for n in 0..3 {
            gone.extend(own.drawn(&icon(OWN_KEPT + 100 + n)));
        }
        assert_eq!(gone.len(), 13, "back to the budget");
        assert!(gone.iter().all(|g| *g != icon(0)), "drawn last frame: kept");
        assert_eq!(own.begin_frame(), gone, "given back when the next begins");
        // One drawn again is no news.
        assert!(own.drawn(&icon(0)).is_empty());
    }

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
    fn an_icon_in_the_color_schemes_colors_is_filled_and_a_symbolic_one_is_a_shape() {
        let dir = tempfile::tempdir().unwrap();
        // Breeze's: a stylesheet whose colors KDE fills.
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
            Pic::Scheme(recolored.clone().into())
        );
        assert_eq!(Pic::of_theme(picture.clone()), Pic::Theme(picture.into()));
        assert_eq!(
            Pic::of_theme(symbolic.clone()),
            Pic::Shape(symbolic.clone().into())
        );
        assert_eq!(Pic::of_theme(png.clone()), Pic::Theme(png.into()));
        // A shape's bytes are kept for `svg()`, which draws it; an icon
        // filled is read where it is drawn.
        let kept = preloaded().lock().unwrap();
        assert!(kept.contains_key(&*symbolic.to_string_lossy()));
        assert!(!kept.contains_key(&*recolored.to_string_lossy()));
    }

    #[test]
    fn a_raster_given_back_leaves_the_last_drawn_once_its_place_is_drawn_anew() {
        use super::{InkColors, SchemeDrawn};
        use crate::theme::Theme;
        use std::sync::Arc;
        let image = || crate::thumbs::owned_render_image(1, 1, vec![0; 4]).unwrap();
        let (light, dark) = (Theme::light(), Theme::dark());
        let in_light = Ink::of(&light, light.text_muted);
        let in_dark = Ink::of(&dark, dark.text_muted);
        let folder: Arc<Path> = Path::new("/icons/folder.svg").into();
        let place = (Arc::clone(&folder), 32);
        let key = |ink| (Arc::clone(&folder), 32, ink);
        let mut drawn = SchemeDrawn::default();

        // Light, then dark shown before it is taken, then light again: the
        // light raster is still GPUI's, so the dark one leaves at once,
        // `last` included.
        let (lit, darkened) = (image(), image());
        drawn.loaded(key(in_light), &lit);
        drawn.loaded(key(in_dark), &darkened);
        assert!(Arc::ptr_eq(&drawn.last[&place], &darkened));
        let given = drawn.settle(&InkColors::of(&light));
        assert_eq!(given.forgotten, [key(in_dark)]);
        assert!(given.dropped.iter().any(|i| Arc::ptr_eq(i, &darkened)));
        assert!(
            Arc::ptr_eq(&drawn.last[&place], &lit),
            "the light one is last"
        );
        assert!(drawn.retiring.is_empty());

        // Dark taken at once: the light raster stays the last, drawn until
        // the dark one is, then leaves the atlas.
        let given = drawn.settle(&InkColors::of(&dark));
        assert_eq!(given.forgotten, [key(in_light)]);
        assert!(given.dropped.is_empty(), "still drawn while dark is made");
        assert!(Arc::ptr_eq(&drawn.retiring[&place], &lit));
        let dark_again = image();
        drawn.loaded(key(in_dark), &dark_again);
        let retired = drawn.replaced(&place, &dark_again).unwrap();
        assert!(Arc::ptr_eq(&retired, &lit));
        assert!(Arc::ptr_eq(&drawn.last[&place], &dark_again));
        assert!(drawn.replaced(&place, &dark_again).is_none());

        // Retired and never drawn again: gone at the next settling.
        let given = drawn.settle(&InkColors::of(&light));
        assert!(given.dropped.is_empty());
        assert!(Arc::ptr_eq(&drawn.retiring[&place], &dark_again));
        let given = drawn.settle(&InkColors::of(&dark));
        assert!(given.dropped.iter().any(|i| Arc::ptr_eq(i, &dark_again)));
        assert!(!drawn.last.contains_key(&place));
        assert!(drawn.retiring.is_empty());
    }

    #[test]
    fn an_ink_is_of_the_colors_of_the_theme_that_draws_it() {
        use super::InkColors;
        use crate::theme::Theme;
        let (light, dark) = (Theme::light(), Theme::dark());
        let colors = InkColors::of(&light);
        for ink in [
            Ink::of(&light, light.text_muted),
            Ink::of(&light, light.text),
            Ink::of(&light, light.selection_text).on(light.selection_bg),
        ] {
            assert!(ink.is_of(&colors), "{ink:?}");
        }
        assert!(!Ink::of(&dark, dark.text_muted).is_of(&colors));
        assert_ne!(InkColors::of(&dark), colors);
        // Fonts and sizes are no colors an ink reads; nor is code's theme.
        let mut written = light.clone();
        written.font = "Another Sans".into();
        written.code_bg = dark.code_bg;
        assert_eq!(InkColors::of(&written), colors);
    }

    #[test]
    fn a_folder_in_the_color_scheme_keeps_its_layers_in_the_themes_colors() {
        // Breeze's 32 px folder, cut down: its body in the accent, a flap
        // darkened over it, an edge in the text's color, faint.
        let dir = tempfile::tempdir().unwrap();
        let folder = dir.path().join("folder.svg");
        std::fs::write(
            &folder,
            r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 4 4"><defs><style type="text/css" id="current-color-scheme">.ColorScheme-Text { color:#31363b; } .ColorScheme-Accent { color:#3daee9; }</style></defs><path style="fill:currentColor" d="M0 0h4v4H0z" class="ColorScheme-Accent"/><path style="fill-opacity:0.5" d="M0 0h4v1H0z"/><path style="fill:currentColor;fill-opacity:0.5" d="M0 3h4v1H0z" class="ColorScheme-Text"/></svg>"#,
        )
        .unwrap();
        let ink = Ink {
            text: [255, 255, 255, 255],
            ground: [0, 0, 0, 255],
            accent: [200, 0, 0, 255],
            positive: [0, 200, 0, 255],
            neutral: [200, 200, 0, 255],
            negative: [200, 0, 200, 255],
        };
        let drawn =
            skwir_platform::icons::rasterize_styled(&folder, 4, Some(&ink.style_sheet())).unwrap();
        let pixel = |y: usize| &drawn.rgba[y * 4 * 4..][..4];
        assert_eq!(pixel(2), [200, 0, 0, 255], "the body in the theme's accent");
        assert_eq!(pixel(0), [100, 0, 0, 255], "the flap darkened over it");
        assert_eq!(
            pixel(3),
            [228, 128, 128, 255],
            "the edge in the text's color, faint"
        );
    }

    #[test]
    fn an_ink_fills_every_part_kdes_scheme_names() {
        let ink = Ink {
            text: [1, 2, 3, 255],
            ground: [4, 5, 6, 255],
            accent: [7, 8, 9, 255],
            positive: [10, 11, 12, 255],
            neutral: [13, 14, 15, 255],
            negative: [16, 17, 18, 128],
        };
        let sheet = ink.style_sheet();
        for (part, color) in [
            ("Text", "rgba(1,2,3,1)"),
            ("ButtonText", "rgba(1,2,3,1)"),
            ("Background", "rgba(4,5,6,1)"),
            ("Accent", "rgba(7,8,9,1)"),
            ("Highlight", "rgba(7,8,9,1)"),
            ("PositiveText", "rgba(10,11,12,1)"),
            ("NeutralText", "rgba(13,14,15,1)"),
            ("NegativeText", "rgba(16,17,18,0.5019608)"),
        ] {
            let rule = sheet
                .split('}')
                .find(|rule| {
                    rule.split('{')
                        .next()
                        .unwrap()
                        .split(',')
                        .any(|s| s == format!(".ColorScheme-{part}"))
                })
                .unwrap_or_else(|| panic!("{part} filled: {sheet}"));
            assert!(
                rule.ends_with(&format!("color:{color} !important")),
                "{rule}"
            );
        }
        assert_eq!(
            ink.tint(),
            gpui::Hsla::from(gpui::Rgba {
                r: 1. / 255.,
                g: 2. / 255.,
                b: 3. / 255.,
                a: 1.
            })
        );
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
