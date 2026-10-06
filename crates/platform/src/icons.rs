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

    use std::path::Path;
    use std::process::Command;
    use std::time::Duration;

    fn gtk_setting(file: &str) -> Option<String> {
        let path = crate::xdg::config_home()?.join(file);
        let text = std::fs::read_to_string(path).ok()?;
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
                let text = std::fs::read_to_string(crate::xdg::config_home()?.join("kdeglobals"));
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
