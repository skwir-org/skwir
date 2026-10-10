//! Icon theme lookup. Cached; never touches the network.
//!
//! A lookup that misses the cache reads the icon themes from disk
//! (`icon_theme`: each theme's index once, and its names through GTK's
//! cache or listed once): it blocks, so it belongs on a worker, never in a
//! render. The UI asks
//! [`try_cached`] while drawing and runs [`lookup`] (or [`for_mime`],
//! [`folder`], [`unknown`], each a few lookups) on a worker for what it
//! did not have; a later [`try_cached`] then answers.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
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

/// The theme the user chose in bibli (`ui.icon_theme`), by its folder's
/// name; `None` follows the desktop.
fn chosen() -> &'static Mutex<Option<String>> {
    static CHOSEN: Mutex<Option<String>> = Mutex::new(None);
    &CHOSEN
}

/// The theme in use, once resolved since the last choice: `None` until a
/// lookup needs it.
fn resolved() -> &'static Mutex<Option<String>> {
    static RESOLVED: Mutex<Option<String>> = Mutex::new(None);
    &RESOLVED
}

/// Advances with each choice: a lookup begun before it keeps its answer to
/// itself, never the cache.
static GENERATION: AtomicU64 = AtomicU64::new(0);

/// Use `theme` (a folder's name, `breeze`), or the desktop's when `None`:
/// the icon theme setting. What was looked up is forgotten, so the next
/// lookups read the new theme. No I/O: callable while drawing.
pub fn choose_theme(theme: Option<&str>) {
    let theme = theme
        .map(str::trim)
        .filter(|t| !t.is_empty())
        .map(str::to_owned);
    if let Ok(mut chosen) = chosen().lock() {
        if *chosen == theme {
            return;
        }
        *chosen = theme;
    }
    GENERATION.fetch_add(1, Ordering::SeqCst);
    if let Ok(mut resolved) = resolved().lock() {
        *resolved = None;
    }
    if let Ok(mut cache) = cache().lock() {
        cache.clear();
    }
    imp::forget_themes();
}

/// The desktop's theme, read once (`imp::detect_theme`).
static DETECTED: OnceLock<String> = OnceLock::new();

/// The active icon theme name: the one chosen in bibli when it is
/// installed, else the desktop's. Reads the icon folders the first time:
/// blocks.
pub fn theme_name() -> String {
    if let Some(name) = resolved().lock().ok().and_then(|r| r.clone()) {
        return name;
    }
    let generation = GENERATION.load(Ordering::SeqCst);
    let chosen = chosen().lock().ok().and_then(|c| c.clone());
    let name = chosen
        .filter(|theme| imp::is_installed(theme))
        .unwrap_or_else(|| DETECTED.get_or_init(imp::detect_theme).clone());
    // Kept unless a choice came meanwhile.
    if let Ok(mut resolved) = resolved().lock()
        && GENERATION.load(Ordering::SeqCst) == generation
    {
        *resolved = Some(name.clone());
    }
    name
}

/// An installed icon theme: its folder's name, which settings and lookups
/// use (`breeze`), and the name it gives itself (`Breeze`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Theme {
    pub folder: String,
    pub name: String,
}

/// The icon themes installed, by name; not the cursor themes, nor hicolor,
/// the base every theme falls back to. Reads the icon folders: blocks.
pub fn installed_themes() -> Vec<Theme> {
    imp::installed_themes()
}

/// Whether the theme `folder` names is installed. Reads the icon folders:
/// blocks.
pub fn is_installed(folder: &str) -> bool {
    imp::is_installed(folder)
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
    let generation = GENERATION.load(Ordering::SeqCst);
    let found = imp::lookup(name, size);
    if let Ok(mut c) = cache().lock()
        && GENERATION.load(Ordering::SeqCst) == generation
    {
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

    use std::path::Path;
    use std::process::Command;
    use std::time::Duration;

    fn gtk_setting(file: &str) -> Option<String> {
        let path = crate::xdg::config_home()?.join(file);
        let text = crate::file::read_regular_text(&path, crate::file::DESKTOP_FILE).ok()?;
        crate::keyfile::entries(&text)
            .find(|(group, key, _)| *group == "Settings" && *key == "gtk-icon-theme-name")
            .map(|(_, _, value)| value.trim().to_owned())
            .filter(|s| !s.is_empty())
    }

    /// Where a desktop keeps the icon theme its user chose. GNOME and the
    /// desktops built on it keep it in `GSettings` and write no
    /// `settings.ini`, KDE in `kdeglobals`, Xfce in xfconf.
    #[derive(Debug, PartialEq, Eq)]
    pub(super) enum Chosen {
        /// `gsettings get <schema> icon-theme`.
        GSettings(&'static str),
        Kde,
        Xfce,
    }

    /// The setting of the first desktop `XDG_CURRENT_DESKTOP` names that
    /// keeps one (`ubuntu:GNOME`, `Budgie:GNOME`, `X-Cinnamon`).
    pub(super) fn chosen_in(desktops: &str) -> Option<Chosen> {
        desktops
            .split(':')
            .find_map(|desktop| match desktop.to_ascii_lowercase().as_str() {
                "gnome" | "gnome-classic" | "gnome-flashback" | "unity" | "budgie"
                | "budgie-desktop" | "pantheon" => {
                    Some(Chosen::GSettings("org.gnome.desktop.interface"))
                }
                "x-cinnamon" | "cinnamon" => {
                    Some(Chosen::GSettings("org.cinnamon.desktop.interface"))
                }
                "mate" => Some(Chosen::GSettings("org.mate.interface")),
                "kde" => Some(Chosen::Kde),
                "xfce" => Some(Chosen::Xfce),
                _ => None,
            })
    }

    /// A string as `gsettings get` prints it (`'Papirus'`).
    pub(super) fn gsettings_string(printed: &str) -> Option<String> {
        let printed = printed.trim();
        let inner = printed
            .strip_prefix('\'')
            .and_then(|s| s.strip_suffix('\''))
            .unwrap_or(printed);
        (!inner.is_empty()).then(|| inner.to_owned())
    }

    /// The theme in `kdeglobals`; Plasma's own, Breeze, when it names none.
    pub(super) fn kde_theme(kdeglobals: Option<&str>) -> String {
        kdeglobals
            .and_then(|text| {
                crate::keyfile::entries(text)
                    .find(|(group, key, _)| *group == "Icons" && *key == "Theme")
                    .map(|(_, _, value)| value.trim().to_owned())
            })
            .filter(|theme| !theme.is_empty())
            .unwrap_or_else(|| "breeze".to_owned())
    }

    /// Where icon themes are installed, as the icon theme specification
    /// lists them: `~/.icons`, then `icons` in each data folder.
    fn theme_folders() -> Vec<PathBuf> {
        bibli_core::os_path::home()
            .map(|home| home.join(".icons"))
            .into_iter()
            .chain(
                crate::xdg::data_search()
                    .into_iter()
                    .map(|data| data.join("icons")),
            )
            .collect()
    }

    /// Whether the theme `name` is installed where themes are.
    pub fn is_installed(name: &str) -> bool {
        installed(&theme_folders(), name)
    }

    /// The icon themes installed where themes are.
    pub fn installed_themes() -> Vec<super::Theme> {
        themes_in(&theme_folders())
    }

    /// The icon themes in `folders`, the first of a name winning (the
    /// user's own before the system's), sorted by name without case. A
    /// theme is a folder with an `index.theme` whose `[Icon Theme]` lists
    /// `Directories`: a cursor theme lists none. Hicolor, the base every
    /// theme falls back to, and `default`, a cursor theme's alias, are left
    /// out.
    pub(super) fn themes_in(folders: &[PathBuf]) -> Vec<super::Theme> {
        let mut found: Vec<super::Theme> = Vec::new();
        for folder in folders {
            let Ok(entries) = std::fs::read_dir(folder) else {
                continue;
            };
            for entry in entries.flatten() {
                let Ok(name) = entry.file_name().into_string() else {
                    continue;
                };
                if matches!(name.as_str(), "hicolor" | "default")
                    || found.iter().any(|theme| theme.folder == name)
                {
                    continue;
                }
                let Ok(text) = crate::file::read_regular_text(
                    &entry.path().join("index.theme"),
                    crate::file::DESKTOP_FILE,
                ) else {
                    continue;
                };
                let field = |wanted: &str| {
                    crate::keyfile::entries(&text)
                        .find(|(group, key, _)| *group == "Icon Theme" && *key == wanted)
                        .map(|(_, _, value)| value.trim().to_owned())
                };
                if field("Directories").is_none_or(|d| d.is_empty()) {
                    continue;
                }
                let shown = field("Name")
                    .filter(|n| !n.is_empty())
                    .unwrap_or_else(|| name.clone());
                found.push(super::Theme {
                    folder: name,
                    name: shown,
                });
            }
        }
        found.sort_by_key(|theme| theme.name.to_lowercase());
        found
    }

    /// Whether the theme `name` (its folder's name, as settings and
    /// lookups name it: `breeze`, never its display name `Breeze`) is
    /// installed in one of `folders`.
    pub(super) fn installed(folders: &[PathBuf], name: &str) -> bool {
        !name.is_empty()
            && !name.contains(['/', '\\'])
            && folders
                .iter()
                .any(|folder| folder.join(name).join("index.theme").is_file())
    }

    /// What `program` prints, run with `args`: a desktop's setting, read in
    /// a moment or not at all.
    pub(super) fn printed(program: &Path, args: &[&str]) -> Option<String> {
        let finished = crate::process::run_capped(
            Command::new(program).args(args),
            None,
            &bibli_core::CancelToken::new(),
            Duration::from_secs(2),
            4096,
        )
        .ok()?;
        finished
            .status
            .success()
            .then(|| String::from_utf8_lossy(&finished.stdout).into_owned())
    }

    /// The theme the user chose in their desktop's own settings.
    fn desktop_setting() -> Option<String> {
        let desktops = std::env::var("XDG_CURRENT_DESKTOP").ok()?;
        match chosen_in(&desktops)? {
            Chosen::GSettings(schema) => gsettings_string(&printed(
                Path::new("gsettings"),
                &["get", schema, "icon-theme"],
            )?),
            Chosen::Kde => {
                let text = crate::file::read_regular_text(
                    &crate::xdg::config_home()?.join("kdeglobals"),
                    crate::file::DESKTOP_FILE,
                );
                Some(kde_theme(text.ok().as_deref()))
            }
            Chosen::Xfce => printed(
                Path::new("xfconf-query"),
                &["-c", "xsettings", "-p", "/Net/IconThemeName"],
            )
            .map(|s| s.trim().to_owned())
            .filter(|s| !s.is_empty()),
        }
    }

    /// The theme the user chose: their desktop's own setting first, then
    /// GTK's (which other desktops and GTK applications outside one read),
    /// each only if installed; else the common themes, else hicolor.
    pub fn detect_theme() -> String {
        let folders = theme_folders();
        let configured = desktop_setting()
            .into_iter()
            .chain(gtk_setting("gtk-4.0/settings.ini"))
            .chain(gtk_setting("gtk-3.0/settings.ini"))
            .collect::<Vec<_>>();
        for candidate in configured
            .iter()
            .map(String::as_str)
            .chain(["Adwaita", "breeze", "Papirus"])
        {
            if installed(&folders, candidate) {
                return candidate.to_owned();
            }
        }
        "hicolor".to_owned()
    }

    type Indexed = std::sync::Mutex<Option<std::sync::Arc<crate::icon_theme::Index>>>;

    /// The themes as indexed: read again after a theme is chosen, so one
    /// installed since is found.
    static INDEX: Indexed = std::sync::Mutex::new(None);

    fn index() -> std::sync::Arc<crate::icon_theme::Index> {
        let mut index = INDEX
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if let Some(index) = index.as_ref() {
            return std::sync::Arc::clone(index);
        }
        let pixmaps = crate::xdg::data_search()
            .into_iter()
            .map(|data| data.join("pixmaps"))
            .chain([PathBuf::from("/usr/share/pixmaps")])
            .collect();
        let made = std::sync::Arc::new(crate::icon_theme::Index::new(theme_folders(), pixmaps));
        *index = Some(std::sync::Arc::clone(&made));
        made
    }

    pub fn forget_themes() {
        *INDEX
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = None;
    }

    /// `name` from the theme in use, its parents and hicolor, else from
    /// the themes most desktops have.
    pub fn lookup(name: &str, size: u16) -> Option<PathBuf> {
        let theme = super::theme_name();
        let mut themes: Vec<&str> = vec![&theme];
        for fallback in ["Adwaita", "breeze"] {
            if fallback != theme {
                themes.push(fallback);
            }
        }
        index().find(&themes, name, size)
    }
}

#[cfg(not(target_os = "linux"))]
mod imp {
    use std::path::PathBuf;

    pub fn detect_theme() -> String {
        String::new()
    }
    pub fn is_installed(_name: &str) -> bool {
        false
    }
    pub fn installed_themes() -> Vec<super::Theme> {
        Vec::new()
    }
    pub fn lookup(_name: &str, _size: u16) -> Option<PathBuf> {
        None
    }
    pub fn forget_themes() {}
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
    use std::path::Path;

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
    fn the_theme_is_read_where_each_desktop_keeps_it() {
        use imp::{Chosen, chosen_in, gsettings_string, kde_theme};
        let gnome = Some(Chosen::GSettings("org.gnome.desktop.interface"));
        assert_eq!(chosen_in("GNOME"), gnome);
        assert_eq!(chosen_in("ubuntu:GNOME"), gnome);
        assert_eq!(chosen_in("Budgie:GNOME"), gnome);
        assert_eq!(
            chosen_in("X-Cinnamon"),
            Some(Chosen::GSettings("org.cinnamon.desktop.interface"))
        );
        assert_eq!(
            chosen_in("MATE"),
            Some(Chosen::GSettings("org.mate.interface"))
        );
        assert_eq!(chosen_in("KDE"), Some(Chosen::Kde));
        assert_eq!(chosen_in("XFCE"), Some(Chosen::Xfce));
        // GTK's settings.ini decides for the others.
        assert_eq!(chosen_in("Hyprland"), None);
        assert_eq!(chosen_in(""), None);

        assert_eq!(
            gsettings_string("'Papirus-Dark'\n").as_deref(),
            Some("Papirus-Dark")
        );
        assert_eq!(gsettings_string("''\n"), None);

        let kdeglobals = "[General]\nColorScheme=BreezeDark\n\n[Icons]\nTheme=Papirus\n";
        assert_eq!(kde_theme(Some(kdeglobals)), "Papirus");
        assert_eq!(
            kde_theme(Some("[General]\nColorScheme=BreezeDark\n")),
            "breeze"
        );
        assert_eq!(kde_theme(None), "breeze");
    }

    #[test]
    fn the_installed_themes_are_the_icon_themes_each_once_by_name() {
        let user = tempfile::tempdir().unwrap();
        let system = tempfile::tempdir().unwrap();
        let theme = |root: &Path, folder: &str, text: &str| {
            std::fs::create_dir_all(root.join(folder)).unwrap();
            std::fs::write(root.join(folder).join("index.theme"), text).unwrap();
        };
        theme(
            user.path(),
            "papirus",
            "[Icon Theme]\nName=Papirus\nDirectories=16x16/apps\n",
        );
        theme(
            user.path(),
            "breeze",
            "[Icon Theme]\nName=Breeze\nDirectories=places/16\n",
        );
        // A cursor theme names no directories.
        theme(
            user.path(),
            "Bibata",
            "[Icon Theme]\nName=Bibata\nInherits=Adwaita\n",
        );
        theme(
            system.path(),
            "breeze",
            "[Icon Theme]\nName=Breeze (system)\nDirectories=x\n",
        );
        theme(
            system.path(),
            "Adwaita",
            "[Icon Theme]\nName=Adwaita\nDirectories=16x16/places\n",
        );
        theme(
            system.path(),
            "hicolor",
            "[Icon Theme]\nName=Hicolor\nDirectories=16x16/apps\n",
        );
        theme(
            system.path(),
            "default",
            "[Icon Theme]\nName=Default\nDirectories=x\n",
        );
        std::fs::create_dir_all(system.path().join("no-index")).unwrap();
        let found = imp::themes_in(&[user.path().to_owned(), system.path().to_owned()]);
        let found: Vec<(&str, &str)> = found
            .iter()
            .map(|t| (t.folder.as_str(), t.name.as_str()))
            .collect();
        assert_eq!(
            found,
            [
                ("Adwaita", "Adwaita"),
                ("breeze", "Breeze"),
                ("papirus", "Papirus")
            ]
        );
    }

    #[test]
    fn a_theme_chosen_in_bibli_is_used_while_it_is_installed() {
        let detected = theme_name();
        // A theme of the test's own, where themes are looked for (the
        // isolated data folder nextest gives each test).
        let data = crate::xdg::data_home().unwrap();
        let folder = format!("bibli-test-theme-{}", std::process::id());
        let theme = data.join("icons").join(&folder);
        std::fs::create_dir_all(&theme).unwrap();
        std::fs::write(
            theme.join("index.theme"),
            "[Icon Theme]\nName=Test\nDirectories=16x16/places\n",
        )
        .unwrap();
        choose_theme(Some(&folder));
        assert_eq!(theme_name(), folder);
        assert!(installed_themes().iter().any(|t| t.folder == folder));
        // One no longer installed follows the desktop.
        choose_theme(Some("bibli-test-absent-theme"));
        assert_eq!(theme_name(), detected);
        choose_theme(None);
        assert_eq!(theme_name(), detected);
        std::fs::remove_dir_all(&theme).unwrap();
    }

    #[test]
    fn a_theme_is_installed_by_its_folders_name_not_its_display_name() {
        let dir = tempfile::tempdir().unwrap();
        let breeze = dir.path().join("icons/breeze");
        std::fs::create_dir_all(&breeze).unwrap();
        std::fs::write(breeze.join("index.theme"), "[Icon Theme]\nName=Breeze\n").unwrap();
        let folders = [dir.path().join("elsewhere"), dir.path().join("icons")];
        assert!(imp::installed(&folders, "breeze"));
        assert!(!imp::installed(&folders, "Breeze"));
        assert!(!imp::installed(&folders, "Papirus"));
        assert!(!imp::installed(&folders, ""));
        assert!(!imp::installed(&folders, "../icons/breeze"));
    }

    #[test]
    fn gsettings_is_asked_as_a_program() {
        use std::os::unix::fs::PermissionsExt as _;
        let dir = tempfile::tempdir().unwrap();
        let fake = dir.path().join("gsettings");
        std::fs::write(
            &fake,
            "#!/bin/sh\n[ \"$*\" = 'get org.gnome.desktop.interface icon-theme' ] && echo \"'Papirus'\"\n",
        )
        .unwrap();
        std::fs::set_permissions(&fake, std::fs::Permissions::from_mode(0o755)).unwrap();
        let printed = imp::printed(&fake, &["get", "org.gnome.desktop.interface", "icon-theme"]);
        assert_eq!(
            printed
                .as_deref()
                .and_then(imp::gsettings_string)
                .as_deref(),
            Some("Papirus")
        );
        // A program that fails, or is not there, says nothing.
        assert_eq!(imp::printed(&fake, &["get", "nothing", "here"]), None);
        assert_eq!(imp::printed(&dir.path().join("absent"), &[]), None);
    }

    #[test]
    fn mime_icon_falls_back() {
        let m = Mime("application/x-bibli-nonexistent".into());
        // Either the generic or last-resort icon must exist.
        assert!(for_mime(&m, 32).is_some());
    }
}
