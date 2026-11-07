//! What a free desktop's own settings say its user chose, where each keeps
//! it: GNOME and the desktops built on it in `GSettings`, Plasma in
//! `kdeglobals`, Xfce in xfconf, and GTK's `settings.ini` for the others
//! and for GTK applications outside one. The icon theme (`icons`) and the
//! interface font (`desktop_style`) are read here alike. Each answer is a
//! program asked or a file read: on a worker, never while drawing.

use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;

/// Where a desktop keeps what its user chose.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum Chosen {
    /// `gsettings get <schema> <key>`.
    GSettings(&'static str),
    /// `kdeglobals`.
    Kde,
    /// xfconf's `xsettings` channel.
    Xfce,
    /// LXQt's `lxqt.conf`.
    Lxqt,
}

/// The settings of the first desktop `desktops` (`XDG_CURRENT_DESKTOP`)
/// names that keeps them (`ubuntu:GNOME`, `Budgie:GNOME`, `X-Cinnamon`).
pub(crate) fn chosen_in(desktops: &str) -> Option<Chosen> {
    desktops
        .split(':')
        .find_map(|desktop| match desktop.to_ascii_lowercase().as_str() {
            "gnome" | "gnome-classic" | "gnome-flashback" | "unity" | "budgie"
            | "budgie-desktop" | "pantheon" => {
                Some(Chosen::GSettings("org.gnome.desktop.interface"))
            }
            "x-cinnamon" | "cinnamon" => Some(Chosen::GSettings("org.cinnamon.desktop.interface")),
            "mate" => Some(Chosen::GSettings("org.mate.interface")),
            "kde" => Some(Chosen::Kde),
            "xfce" => Some(Chosen::Xfce),
            "lxqt" => Some(Chosen::Lxqt),
            _ => None,
        })
}

/// The settings of the desktop this session runs.
pub(crate) fn chosen() -> Option<Chosen> {
    chosen_in(&std::env::var("XDG_CURRENT_DESKTOP").ok()?)
}

/// A string as `gsettings get` prints it (`'Papirus'`).
pub(crate) fn gsettings_string(printed: &str) -> Option<String> {
    let printed = printed.trim();
    let inner = printed
        .strip_prefix('\'')
        .and_then(|s| s.strip_suffix('\''))
        .unwrap_or(printed);
    (!inner.is_empty()).then(|| inner.to_owned())
}

/// What `program` prints, run with `args`: a desktop's setting, read in a
/// moment or not at all.
pub(crate) fn printed(program: &Path, args: &[&str]) -> Option<String> {
    let finished = crate::process::run_capped(
        Command::new(program).args(args),
        None,
        &skwir_core::CancelToken::new(),
        Duration::from_secs(2),
        4096,
    )
    .ok()?;
    finished
        .status
        .success()
        .then(|| String::from_utf8_lossy(&finished.stdout).into_owned())
}

/// A string `GSettings` keeps.
pub(crate) fn gsettings(schema: &str, key: &str) -> Option<String> {
    gsettings_string(&printed(Path::new("gsettings"), &["get", schema, key])?)
}

/// A property of xfconf's `xsettings` channel (`/Net/IconThemeName`).
pub(crate) fn xfconf(property: &str) -> Option<String> {
    printed(
        Path::new("xfconf-query"),
        &["-c", "xsettings", "-p", property],
    )
    .map(|s| s.trim().to_owned())
    .filter(|s| !s.is_empty())
}

/// A configuration file of `relative` in the user's configuration folder,
/// as text.
fn config_text(relative: &str) -> Option<String> {
    crate::file::read_regular_text(
        &crate::xdg::config_home()?.join(relative),
        crate::file::DESKTOP_FILE,
    )
    .ok()
}

/// LXQt's `lxqt.conf`, as text.
pub(crate) fn lxqt_conf() -> Option<String> {
    config_text("lxqt/lxqt.conf")
}

/// The settings of the Qt platform theme that gives Qt's applications
/// their fonts and colors outside Plasma and LXQt, as
/// `QT_QPA_PLATFORMTHEME` names it (ADR-071, amended 2026-11-07): qt5ct's
/// or qt6ct's (the other's when it has none), qtengine's or
/// hyprqt6engine's, each found where the program looks for it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct QtSettings {
    /// `qt5ct`, `qt6ct`, `qtengine` or `hyprqt6engine`.
    pub(crate) program: &'static str,
    pub(crate) path: PathBuf,
    pub(crate) text: String,
}

/// Where the Qt platform themes look for their settings.
#[derive(Clone, Debug, Default)]
pub(crate) struct QtFolders {
    pub(crate) config_home: Option<PathBuf>,
    /// `$HOME/.config`, which qtengine and hyprqt6engine look in after
    /// `XDG_CONFIG_HOME`.
    pub(crate) home_config: Option<PathBuf>,
    pub(crate) config_dirs: Vec<PathBuf>,
    /// `QTENGINE_CONFIG`, which qtengine reads first.
    pub(crate) qtengine: Option<PathBuf>,
}

impl QtFolders {
    fn of_session() -> Self {
        QtFolders {
            config_home: crate::xdg::config_home(),
            home_config: skwir_core::os_path::home().map(|home| home.join(".config")),
            config_dirs: crate::xdg::config_dirs(),
            qtengine: std::env::var_os("QTENGINE_CONFIG")
                .map(PathBuf::from)
                .filter(|path| path.is_absolute()),
        }
    }

    /// The files `program` reads its settings from, in the order it looks:
    /// qt5ct and qt6ct the configuration folder's alone; qtengine
    /// `QTENGINE_CONFIG`, then the configuration folders
    /// (`configmanager.cpp`); hyprqt6engine those folders' `hypr`, then
    /// `/etc/xdg`'s (hyprutils' `findConfig`).
    fn candidates(&self, program: &str) -> Vec<PathBuf> {
        let folders = || {
            self.config_home
                .iter()
                .chain(&self.home_config)
                .chain(&self.config_dirs)
                .cloned()
        };
        match program {
            "qtengine" => self
                .qtengine
                .iter()
                .cloned()
                .chain(folders().map(|folder| folder.join("qtengine/config.json")))
                .collect(),
            "hyprqt6engine" => folders()
                .chain(std::iter::once(PathBuf::from("/etc/xdg")))
                .map(|folder| folder.join("hypr/hyprqt6engine.conf"))
                .collect(),
            ct => self
                .config_home
                .iter()
                .map(|folder| folder.join(format!("{ct}/{ct}.conf")))
                .collect(),
        }
    }
}

/// The Qt platform theme's settings this session names.
pub(crate) fn qt_settings() -> Option<QtSettings> {
    let named = std::env::var("QT_QPA_PLATFORMTHEME").ok()?;
    qt_settings_in(&named, &QtFolders::of_session())
}

/// The settings of the platform theme `named` (`QT_QPA_PLATFORMTHEME`'s
/// value), found in `folders`.
pub(crate) fn qt_settings_in(named: &str, folders: &QtFolders) -> Option<QtSettings> {
    let programs: &[&'static str] = match named.trim().to_ascii_lowercase().as_str() {
        "qt6ct" => &["qt6ct", "qt5ct"],
        "qt5ct" => &["qt5ct", "qt6ct"],
        "qtengine" => &["qtengine"],
        "hyprqt6engine" => &["hyprqt6engine"],
        _ => return None,
    };
    programs.iter().find_map(|&program| {
        folders.candidates(program).into_iter().find_map(|path| {
            let text = crate::file::read_regular_text(&path, crate::file::DESKTOP_FILE).ok()?;
            Some(QtSettings {
                program,
                path,
                text,
            })
        })
    })
}

/// The value of `key` (`theme:color_scheme`) in a Hyprland ecosystem
/// program's settings (`hyprlang`), the last that names it: written in a
/// category's block (`theme { color_scheme = … }`) or by its whole name;
/// `#` begins a comment, `##` writes one. Variables and `source`d files
/// are not followed.
pub(crate) fn hyprlang_value(text: &str, key: &str) -> Option<String> {
    let mut open: Vec<String> = Vec::new();
    let mut found = None;
    for line in text.lines() {
        let mut kept = String::new();
        let mut chars = line.chars().peekable();
        while let Some(c) = chars.next() {
            if c == '#' {
                if chars.peek() == Some(&'#') {
                    chars.next();
                } else {
                    break;
                }
            }
            kept.push(c);
        }
        let line = kept.trim();
        if line == "}" {
            open.pop();
        } else if let Some(category) = line.strip_suffix('{') {
            open.push(category.trim().to_owned());
        } else if let Some((name, value)) = line.split_once('=') {
            let whole: Vec<&str> = open
                .iter()
                .map(String::as_str)
                .chain(std::iter::once(name.trim()))
                .collect();
            if whole.join(":") == key {
                found = Some(value.trim().to_owned());
            }
        }
    }
    found.filter(|value| !value.is_empty())
}

/// Plasma's `kdeglobals` as `KConfig` cascades them, each as text: the
/// user's, then those of `XDG_CONFIG_DIRS` (the system's, and the global
/// theme's defaults, `kdedefaults`, which Plasma names there).
pub(crate) fn kdeglobals_cascade() -> Vec<String> {
    crate::xdg::config_home()
        .into_iter()
        .chain(crate::xdg::config_dirs())
        .filter_map(|config| {
            crate::file::read_regular_text(&config.join("kdeglobals"), crate::file::DESKTOP_FILE)
                .ok()
        })
        .collect()
}

/// Plasma's `kdeglobals`, as text.
pub(crate) fn kdeglobals() -> Option<String> {
    crate::file::read_regular_text(
        &crate::xdg::config_home()?.join("kdeglobals"),
        crate::file::DESKTOP_FILE,
    )
    .ok()
}

/// The GTK3 theme `GTK_THEME` forces on GTK's applications (`Name`, or
/// `Name:dark`), whatever their settings say.
pub(crate) fn forced_gtk_theme() -> Option<String> {
    std::env::var("GTK_THEME")
        .ok()
        .map(|name| name.trim().to_owned())
        .filter(|name| !name.is_empty())
}

/// The GTK3 theme the desktop's own settings name (GNOME's, MATE's,
/// Cinnamon's, Xfce's); Plasma's and LXQt's colors are their own, so they
/// name none here.
pub(crate) fn own_gtk_theme() -> Option<String> {
    match chosen() {
        Some(Chosen::GSettings(schema)) => gsettings(schema, "gtk-theme"),
        Some(Chosen::Xfce) => xfconf("/Net/ThemeName"),
        Some(Chosen::Kde | Chosen::Lxqt) | None => None,
    }
}

/// `key` of `group` in a key file's `text`, when it says something.
pub(crate) fn keyfile_value(text: &str, group: &str, key: &str) -> Option<String> {
    crate::keyfile::entries(text)
        .find(|(g, k, _)| *g == group && *k == key)
        .map(|(_, _, value)| value.trim().to_owned())
        .filter(|value| !value.is_empty())
}

/// `key` of GTK's `settings.ini`s, GTK 4's first, then GTK 3's.
pub(crate) fn gtk_settings(key: &str) -> Vec<String> {
    ["gtk-4.0/settings.ini", "gtk-3.0/settings.ini"]
        .iter()
        .filter_map(|file| {
            let path = crate::xdg::config_home()?.join(file);
            let text = crate::file::read_regular_text(&path, crate::file::DESKTOP_FILE).ok()?;
            keyfile_value(&text, "Settings", key)
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn each_desktop_s_settings_are_read_where_it_keeps_them() {
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
        assert_eq!(chosen_in("LXQt"), Some(Chosen::Lxqt));
        // GTK's settings.ini, or the portal, decides for the others.
        assert_eq!(chosen_in("Hyprland"), None);
        assert_eq!(chosen_in(""), None);

        assert_eq!(
            gsettings_string("'Papirus-Dark'\n").as_deref(),
            Some("Papirus-Dark")
        );
        assert_eq!(gsettings_string("''\n"), None);

        let text = "[General]\nfont=Noto Sans,10,-1,5,400,0,0,0,0,0\n\n[Icons]\nTheme=\n";
        assert_eq!(
            keyfile_value(text, "General", "font").as_deref(),
            Some("Noto Sans,10,-1,5,400,0,0,0,0,0")
        );
        assert_eq!(keyfile_value(text, "Icons", "Theme"), None);
        assert_eq!(keyfile_value(text, "Icons", "Absent"), None);
    }

    #[test]
    fn a_qt_platform_theme_s_settings_are_found_where_it_looks() {
        let dir = tempfile::tempdir().unwrap();
        let at = |relative: &str| dir.path().join(relative);
        let write = |relative: &str, text: &str| {
            std::fs::create_dir_all(at(relative).parent().unwrap()).unwrap();
            std::fs::write(at(relative), text).unwrap();
        };
        let folders = QtFolders {
            config_home: Some(at("config")),
            home_config: Some(at("home/.config")),
            config_dirs: vec![at("system")],
            qtengine: None,
        };
        let found = |named: &str, folders: &QtFolders| {
            qt_settings_in(named, folders).map(|s| (s.program, s.path))
        };
        // Nothing written: none, and none for a theme not read here.
        assert_eq!(found("qt6ct", &folders), None);
        write("config/qt6ct/qt6ct.conf", "[Appearance]\n");
        write("config/kvantum/kvantum.kvconfig", "");
        assert_eq!(found("gtk3", &folders), None);
        // qt5ct named, qt6ct's taken where qt5ct has none; its own first.
        assert_eq!(
            found("qt5ct", &folders),
            Some(("qt6ct", at("config/qt6ct/qt6ct.conf")))
        );
        write("config/qt5ct/qt5ct.conf", "[Appearance]\n");
        assert_eq!(found(" QT5CT ", &folders).unwrap().0, "qt5ct");
        // qtengine and hyprqt6engine in the system's folders, then the
        // user's, then (qtengine) the file QTENGINE_CONFIG names.
        write("system/qtengine/config.json", "{}");
        write("system/hypr/hyprqt6engine.conf", "");
        assert_eq!(
            found("qtengine", &folders),
            Some(("qtengine", at("system/qtengine/config.json")))
        );
        assert_eq!(
            found("hyprqt6engine", &folders),
            Some(("hyprqt6engine", at("system/hypr/hyprqt6engine.conf")))
        );
        write("home/.config/qtengine/config.json", "{}");
        write("config/hypr/hyprqt6engine.conf", "");
        assert_eq!(
            found("qtengine", &folders).unwrap().1,
            at("home/.config/qtengine/config.json")
        );
        assert_eq!(
            found("hyprqt6engine", &folders).unwrap().1,
            at("config/hypr/hyprqt6engine.conf")
        );
        write("elsewhere.json", "{}");
        let named = QtFolders {
            qtengine: Some(at("elsewhere.json")),
            ..folders.clone()
        };
        assert_eq!(found("qtengine", &named).unwrap().1, at("elsewhere.json"));
    }

    #[test]
    fn hyprlang_s_values_are_read_by_their_whole_name() {
        let text = "general {\n  gaps = 4\n}\n\
                    theme {\n  font = Noto Sans   # the interface's\n  font_size = 11\n\
                      nested {\n    key = deep\n  }\n  color = ##fff\n}\n\
                    theme:font_size = 12\n# theme:font = commented\n";
        assert_eq!(
            hyprlang_value(text, "theme:font").as_deref(),
            Some("Noto Sans")
        );
        assert_eq!(
            hyprlang_value(text, "theme:font_size").as_deref(),
            Some("12")
        );
        assert_eq!(
            hyprlang_value(text, "theme:nested:key").as_deref(),
            Some("deep")
        );
        assert_eq!(hyprlang_value(text, "theme:color").as_deref(), Some("#fff"));
        assert_eq!(hyprlang_value(text, "general:gaps").as_deref(), Some("4"));
        assert_eq!(hyprlang_value(text, "gaps"), None);
        assert_eq!(hyprlang_value("theme {\n  font =\n}\n", "theme:font"), None);
    }

    #[cfg(unix)]
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
        let said = printed(&fake, &["get", "org.gnome.desktop.interface", "icon-theme"]);
        assert_eq!(
            said.as_deref().and_then(gsettings_string).as_deref(),
            Some("Papirus")
        );
        // A program that fails, or is not there, says nothing.
        assert_eq!(printed(&fake, &["get", "nothing", "here"]), None);
        assert_eq!(printed(&dir.path().join("absent"), &[]), None);
    }
}
