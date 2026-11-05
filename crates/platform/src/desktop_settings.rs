//! What a free desktop's own settings say its user chose, where each keeps
//! it: GNOME and the desktops built on it in `GSettings`, Plasma in
//! `kdeglobals`, Xfce in xfconf, and GTK's `settings.ini` for the others
//! and for GTK applications outside one. The icon theme (`icons`) and the
//! interface font (`desktop_style`) are read here alike. Each answer is a
//! program asked or a file read: on a worker, never while drawing.

use std::path::Path;
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

/// The file Qt's applications take their fonts from outside Plasma and
/// LXQt, as `QT_QPA_PLATFORMTHEME` names its program (`qt5ct`, `qt6ct`),
/// the other's when it has none: the program's name and the file's text.
pub(crate) fn qt_ct() -> Option<(&'static str, String)> {
    let named = std::env::var("QT_QPA_PLATFORMTHEME").ok()?;
    let order: [&'static str; 2] = match named.trim().to_ascii_lowercase().as_str() {
        "qt6ct" => ["qt6ct", "qt5ct"],
        "qt5ct" => ["qt5ct", "qt6ct"],
        _ => return None,
    };
    order
        .into_iter()
        .find_map(|program| Some((program, config_text(&format!("{program}/{program}.conf"))?)))
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
