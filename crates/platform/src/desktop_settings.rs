//! What a free desktop's own settings say its user chose, where each keeps
//! it: GNOME and the desktops built on it in `GSettings`, Plasma in
//! `kdeglobals`, Xfce in xfconf, and GTK's `settings.ini` for the others
//! and for GTK applications outside one. The icon theme (`icons`) and the
//! interface font (`desktop_style`) are read here alike. Each answer is a
//! program asked or a file read: on a worker, never while drawing.

use std::borrow::Cow;
use std::path::{Path, PathBuf};
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
        crate::process::command(program).args(args),
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

/// Plasma's `kdeglobals` as `KConfig` cascades them, each file's text,
/// the most specific first (`kdeglobals_cascade_in`, in the session's
/// folders).
pub(crate) fn kdeglobals_cascade() -> Vec<String> {
    // `KConfig` reads no `/etc/kde5rc` where `KDE_SKIP_KDERC` is set.
    let kderc = std::env::var_os("KDE_SKIP_KDERC")
        .is_none()
        .then(|| PathBuf::from("/etc/kde5rc"));
    kdeglobals_cascade_in(
        crate::xdg::config_home().as_deref(),
        &crate::xdg::config_dirs(),
        kderc,
    )
}

/// The files `KConfig` reads as `kdeglobals`
/// (`KConfigPrivate::ensureGlobalFilesAreInitialized`), each as text, the
/// most specific first, as `kconfig_value` takes them: the user's
/// `kdeglobals`, then the user's `system.kdeglobals` (`config_home`), the
/// `kdeglobals` of the system's folders (`config_dirs`, in their order:
/// the global theme's defaults, `kdedefaults`, which Plasma names there,
/// and the system's), their `system.kdeglobals`, and last `kderc`,
/// `/etc/kde5rc` (the name KDE Frameworks 6 reads still), where an
/// administrator may keep Kiosk's locks. A file not there, or not
/// readable, is passed over, as `KConfig` passes it over.
fn kdeglobals_cascade_in(
    config_home: Option<&Path>,
    config_dirs: &[PathBuf],
    kderc: Option<PathBuf>,
) -> Vec<String> {
    let in_dirs = |name: &'static str| config_dirs.iter().map(move |dir| dir.join(name));
    config_home
        .iter()
        .flat_map(|home| [home.join("kdeglobals"), home.join("system.kdeglobals")])
        .chain(in_dirs("kdeglobals"))
        .chain(in_dirs("system.kdeglobals"))
        .chain(kderc)
        .filter_map(|file| crate::file::read_regular_text(&file, crate::file::DESKTOP_FILE).ok())
        .collect()
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

/// `key` of `group` in a cascade of `KConfig` files (each file's text,
/// the most specific first, the user's before the system's and
/// `/etc/kde5rc`: `kdeglobals_cascade`), as `KConfig` reads them (KDE
/// Frameworks' `KConfigIniBackend::parseConfig`, `KEntryMap::setEntry`, and
/// `KConfigPrivate`'s reading of the global files): from the least
/// specific to the user's, each saying it replacing what one before
/// said, but where a file locks it (Kiosk's `[$i]`: after the key, after
/// its group's name, or alone on a line, locking the whole file),
/// whereupon no file after it changes it: an administrator's word over
/// the user's. A key deleted (`key[$d]`, with or without `=`) says
/// nothing again. Names, keys and values are read with the escapes
/// `KConfig` writes (`kconfig_unescaped`), a value said `key[$e]` expanded
/// as `KConfig` expands it (`kconfig_expanded`) from Skwir's environment
/// (`kconfig_variable`). A value empty once read so leaves what was said
/// before it, as `keyfile_value` takes none; a localized one (`key[fr]`)
/// is not read.
pub(crate) fn kconfig_value(cascade: &[impl AsRef<str>], group: &str, key: &str) -> Option<String> {
    kconfig_value_with(cascade, group, key, &kconfig_variable)
}

/// `kconfig_value`, a `[$e]` value expanded with what `variable` gives.
fn kconfig_value_with(
    cascade: &[impl AsRef<str>],
    group: &str,
    key: &str,
    variable: &dyn Fn(&str) -> Option<String>,
) -> Option<String> {
    let mut value = None;
    for text in cascade.iter().rev() {
        if kconfig_read(text.as_ref(), group, key, variable, &mut value) {
            break;
        }
    }
    value
}

/// Reads one `KConfig` file's word on `key` in `group` into `value`, as
/// `KConfig` takes each line in turn, and says whether no file after it
/// may change it: the file locked (`[$i]` alone on a line, wherever it
/// is; every group after it in the file locked too), the group (`[$i]`
/// last after its name; `KConfig` marks the group once the file is read,
/// so what the file says of the key under another header of it still
/// counts), or the key (`[$i]` among its options, or said under a locked
/// header), whereupon no later line of the file changes it either.
/// (`KConfig` lets a later file of defaults, though not the user's
/// `kdeglobals`, change a key a locked group holds unlocked; here no
/// later file does. It also stops reading a file at its hundredth line
/// in error; here every line is read.)
fn kconfig_read(
    text: &str,
    group: &str,
    key: &str,
    variable: &dyn Fn(&str) -> Option<String>,
    value: &mut Option<String>,
) -> bool {
    let (mut file_locked, mut header_locked) = (false, false);
    let (mut group_locked, mut key_locked) = (false, false);
    let mut in_group = false;
    for line in text.lines() {
        let line = kconfig_trim(line);
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        if line.starts_with('[') {
            header_locked = file_locked;
            // A header `KConfig` passes over leaves the group read as it was.
            let Some((name, locks)) = kconfig_group(line) else {
                continue;
            };
            if locks && name.is_empty() {
                file_locked = true;
            } else if locks {
                header_locked = true;
            }
            in_group = name == group;
            group_locked |= in_group && header_locked;
            continue;
        }
        if !in_group || key_locked {
            continue;
        }
        let Some(entry) = kconfig_entry(line) else {
            continue;
        };
        if kconfig_unescaped(entry.key) != key {
            continue;
        }
        let said = entry.said.map(|said| {
            let said = kconfig_unescaped(said);
            if entry.expands {
                kconfig_expanded(&said, variable)
            } else {
                said.into_owned()
            }
        });
        match said {
            None => *value = None,
            Some(said) if !said.is_empty() => *value = Some(said),
            Some(_) => {}
        }
        key_locked = entry.locks || header_locked;
    }
    file_locked || group_locked || key_locked
}

/// What `KConfig` trims from a line, a key and a value
/// (`QByteArrayView::trimmed`): ASCII's spaces alone, never a no-break
/// space.
fn kconfig_trim(text: &str) -> &str {
    text.trim_matches([' ', '\t', '\n', '\x0b', '\x0c', '\r'])
}

/// A group's name, a key or a value as `KConfig` reads the escapes it
/// writes (`KConfigIniBackend::printableToString`): `\s` a space, `\t`,
/// `\n`, `\r` and `\\` what they stand for, `\xHH` the byte `HH` (an `x`
/// where those two are not hex digits; where fewer follow, an `x` ending
/// the text), `\;` and `\,` kept whole (a desktop file's), a `\` ending
/// the text left out. Text holding a `\` is trimmed first. At any other
/// escape `KConfig` stops, leaving a `\` and after it the text as written
/// from the place it reached (`\s\q` reads ` \\q`): so does this.
fn kconfig_unescaped(text: &str) -> Cow<'_, str> {
    if !text.contains('\\') {
        return Cow::Borrowed(text);
    }
    let hex = |byte: u8| match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        b'A'..=b'F' => Some(byte - b'A' + 10),
        _ => None,
    };
    let written = kconfig_trim(text).as_bytes();
    let mut read = Vec::with_capacity(written.len());
    let mut at = 0;
    while let Some(&byte) = written.get(at) {
        at += 1;
        if byte != b'\\' {
            read.push(byte);
            continue;
        }
        let Some(&escaped) = written.get(at) else {
            break;
        };
        at += 1;
        match escaped {
            b's' => read.push(b' '),
            b't' => read.push(b'\t'),
            b'n' => read.push(b'\n'),
            b'r' => read.push(b'\r'),
            b'\\' => read.push(b'\\'),
            b';' | b',' => read.extend([b'\\', escaped]),
            b'x' => {
                let Some(&[high, low]) = written.get(at..at + 2) else {
                    read.push(b'x');
                    break;
                };
                read.push(match (hex(high), hex(low)) {
                    (Some(high), Some(low)) => (high << 4) | low,
                    _ => b'x',
                });
                at += 2;
            }
            _ => {
                read.push(b'\\');
                let reached = read.len();
                read.extend_from_slice(&written[reached..]);
                break;
            }
        }
    }
    Cow::Owned(String::from_utf8_lossy(&read).into_owned())
}

/// A value said `key[$e]=…` as `KConfig` expands it when it is read
/// (`KConfigPrivate::expandString`): `$NAME` (letters, digits and `_`)
/// and `${NAME}` what `variable` gives for `NAME`, nothing where it gives
/// none, `$$` a `$`, a `$` ending the value kept; any other `$` is left
/// out, so `$(command)` runs nothing, as `KConfig` runs none. What a
/// variable gives is not expanded again. (`KConfig` asks for a name in
/// Latin-1; here as written, the same for every ASCII name.)
fn kconfig_expanded(value: &str, variable: &dyn Fn(&str) -> Option<String>) -> String {
    let mut expanded = String::with_capacity(value.len());
    let mut rest = value;
    while let Some(at) = rest.find('$') {
        expanded.push_str(&rest[..at]);
        let after = &rest[at + 1..];
        rest = if after.is_empty() {
            expanded.push('$');
            after
        } else if let Some(after) = after.strip_prefix('$') {
            expanded.push('$');
            after
        } else {
            let (name, next) = match after.strip_prefix('{') {
                Some(braced) => braced.split_once('}').unwrap_or((braced, "")),
                None => after.split_at(
                    after
                        .find(|c: char| !(c.is_alphanumeric() || c == '_'))
                        .unwrap_or(after.len()),
                ),
            };
            if !name.is_empty() {
                expanded.push_str(&variable(name).unwrap_or_default());
            }
            next
        };
    }
    expanded.push_str(rest);
    expanded
}

/// A variable as `KConfig` expands it, from Skwir's environment: its
/// value where it is set and not empty, else for `QT_DATA_HOME`,
/// `QT_CONFIG_HOME` and `QT_CACHE_HOME` the user's folders (Qt's writable
/// locations), else none.
fn kconfig_variable(name: &str) -> Option<String> {
    std::env::var_os(name)
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
        .or_else(|| match name {
            "QT_DATA_HOME" => crate::xdg::data_home(),
            "QT_CONFIG_HOME" => crate::xdg::config_home(),
            "QT_CACHE_HOME" => crate::xdg::cache_home(),
            _ => None,
        })
        .map(|value| value.to_string_lossy().into_owned())
}

/// A group's header as `KConfig` reads it: the group's name (its parts
/// read with their escapes, `kconfig_unescaped`, and joined as `KConfig`
/// joins them, by U+001D: `[A][B]`) and whether `[$i]` ends it, alone
/// (the file locked) or after a name (the group). `$i` locks only as the
/// last part and alone, as written: any other part is the name's
/// (`[R][$ie]` names a group below `R`). What follows a `]` but another
/// `[` is passed over. `None` for a `[` unclosed, a header `KConfig`
/// passes over.
fn kconfig_group(line: &str) -> Option<(String, bool)> {
    let (mut name, mut locks) = (String::new(), false);
    let mut rest = line;
    while let Some(part) = rest.strip_prefix('[') {
        let end = part.find(']')?;
        let (part, after) = (&part[..end], &part[end + 1..]);
        if part == "$i" && after.is_empty() {
            locks = true;
        } else {
            if !name.is_empty() {
                name.push('\u{1d}');
            }
            name.push_str(&kconfig_unescaped(part));
        }
        rest = after;
    }
    Some((name, locks))
}

/// One line of a group as `KConfig` reads it, its key and value as
/// written (`kconfig_unescaped` reads them).
struct KconfigEntry<'a> {
    /// The key, without its options and locale.
    key: &'a str,
    /// Its value, `None` where the line deletes it (`[$d]`).
    said: Option<&'a str>,
    /// Whether the line locks it (`[$i]`).
    locks: bool,
    /// Whether its value is expanded (`[$e]`).
    expands: bool,
}

/// The entry of `line` as `KConfig` reads it: the brackets after its key
/// from the last to the first, one starting with `$` its options, each
/// letter in turn (`i` locks; `e` expands the value; `d` deletes the key
/// named before that bracket at once, `=` or not, locked only by what was
/// read before it: `[$id]` locks, `[$di]` does not), any other its
/// locale. `None` for a line it passes over (no `=` where nothing is
/// deleted, a `[` unclosed, two locales) or a value of a locale's
/// (`key[fr]`), which Skwir does not read.
fn kconfig_entry(line: &str) -> Option<KconfigEntry<'_>> {
    let (mut key, value) = match line.split_once('=') {
        Some((key, value)) => (kconfig_trim(key), Some(kconfig_trim(value))),
        None => (line, None),
    };
    let (mut locks, mut expands, mut localized) = (false, false, false);
    while let Some(start) = key.rfind('[') {
        let end = start + key[start..].find(']')?;
        let part = &key[start + 1..end];
        if let Some(options) = part.strip_prefix('$') {
            for option in options.chars() {
                match option {
                    'i' => locks = true,
                    'e' => expands = true,
                    'd' => {
                        return Some(KconfigEntry {
                            key: &key[..start],
                            said: None,
                            locks,
                            expands,
                        });
                    }
                    _ => {}
                }
            }
        } else if std::mem::replace(&mut localized, true) {
            return None;
        }
        key = &key[..start];
    }
    let value = value?;
    (!localized).then_some(KconfigEntry {
        key,
        said: Some(value),
        locks,
        expands,
    })
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

    /// A cascade of `KConfig` files is read from the system's to the
    /// user's, a later one's word over an earlier's, but where an earlier
    /// one locks it: the key, its group or the whole file.
    #[test]
    fn kconfig_files_are_read_as_kconfig_cascades_them() {
        let value = |cascade: &[&str]| kconfig_value(cascade, "Recent", "Use");
        let user = "[Recent]\nUse=true\n";
        assert_eq!(
            value(&[user, "[Recent]\nUse=false\n"]).as_deref(),
            Some("true")
        );
        assert_eq!(
            value(&["", "[Recent]\nUse=false\n"]).as_deref(),
            Some("false")
        );
        for lock in [
            "[Recent][$i]\nUse=false\n",
            "[Recent]\nUse[$i]=false\n",
            "[Recent]\nUse[$ie]=false\n",
            "[$i]\n[Recent]\nUse=false\n",
            // `[$i]` alone locks the whole file wherever it is.
            "[Recent]\nUse=false\n[$i]\n",
            // A group's lock later in the file: KConfig takes its value.
            "[Recent]\nUse=true\n[Recent][$i]\nUse=false\n",
        ] {
            assert_eq!(value(&[user, lock]).as_deref(), Some("false"), "{lock}");
        }
        // A locked header before an unlocked one of the group: the first.
        assert_eq!(
            value(&[user, "[Recent][$i]\nUse=false\n[Recent]\nUse=true\n"]).as_deref(),
            Some("false")
        );
        // `$i` locks a group only alone and last: `[Recent][$ie]` names
        // a group below it, `[Recent][$i][x]` another.
        for other in ["[Recent][$ie]\nUse=false\n", "[Recent][$i][x]\nUse=false\n"] {
            assert_eq!(value(&[user, other]).as_deref(), Some("true"), "{other}");
        }
        // The lock nearest the system's wins; what it locks it need not say.
        let locked = "[Recent][$i]\nUse=false\n";
        let look = "[Recent][$i]\nUse=true\n";
        assert_eq!(value(&[user, locked, look]).as_deref(), Some("true"));
        assert_eq!(value(&[user, "[Recent][$i]\nOther=1\n"]), None);
        // Another group's lock, a locale's value and an empty value say
        // nothing of it; one deleted takes back what a file before said.
        assert_eq!(
            value(&[user, "[Other][$i]\nUse=false\n"]).as_deref(),
            Some("true")
        );
        assert_eq!(value(&["[Recent]\nUse[fr]=non\n"]), None);
        assert_eq!(
            value(&["[Recent]\nUse=\n", "[Recent]\nUse=false\n"]).as_deref(),
            Some("false")
        );
        // Deleted as KConfig writes it (`Use[$d]`, no `=`) or with one.
        for deleted in ["[Recent]\nUse[$d]\n", "[Recent]\nUse[$d]=\n"] {
            assert_eq!(
                value(&[deleted, "[Recent]\nUse=false\n"]),
                None,
                "{deleted}"
            );
        }
        // Its letters in turn: `[$id]` deletes it locked, `[$di]` at its
        // `d`, unlocked, as KConfig reads it.
        let system = "[Recent]\nUse=false\n";
        assert_eq!(value(&[user, "[Recent]\nUse[$id]\n", system]), None);
        assert_eq!(
            value(&[user, "[Recent]\nUse[$di]\n", system]).as_deref(),
            Some("true")
        );
        // A key with no `=` that deletes nothing, a header unclosed: passed
        // over, the group read before it read on.
        assert_eq!(
            value(&["[Recent]\nUse\n", system]).as_deref(),
            Some("false")
        );
        assert_eq!(value(&["[Recent]\n[Other\nUse=2\n"]).as_deref(), Some("2"));
        // Within a file, the last said, unless a lock came first.
        assert_eq!(value(&["[Recent]\nUse=1\nUse=2\n"]).as_deref(), Some("2"));
        assert_eq!(
            value(&["[Recent]\nUse[$i]=1\nUse=2\n"]).as_deref(),
            Some("1")
        );
    }

    /// Names, keys and values are read with the escapes `KConfig` writes,
    /// as its `printableToString` reads them, and a `[$e]` value is
    /// expanded as its `expandString` expands it, from the environment
    /// alone: no command is run.
    #[test]
    fn kconfig_s_escapes_and_expansions_are_read_as_it_reads_them() {
        let said = |line: &str| kconfig_value(&[format!("[Icons]\n{line}\n")], "Icons", "Theme");
        for (line, read) in [
            (r"Theme=Breeze\sDark", "Breeze Dark"),
            (r"Theme=\sspaced\s", " spaced "),
            (r"Theme=a\tb\nc\rd\\e", "a\tb\nc\rd\\e"),
            (r"Theme=caf\xc3\xA9", "café"),
            (r"Theme=a\;b\,c", r"a\;b\,c"),
            // An `x` where no hex digits follow; ending the text where
            // fewer than two do; a `\` ending it left out.
            (r"Theme=a\xzzb", "axb"),
            (r"Theme=ab\x4", "abx"),
            (r"Theme=ab\", "ab"),
            // An unknown escape: `KConfig` stops there, a `\` and the
            // text as written from the place it reached following.
            (r"Theme=\s\q", r" \\q"),
            (r"Theme=ab\qcd", r"ab\qcd"),
            // Only ASCII's spaces are trimmed, as `KConfig` trims them.
            ("Theme=Breeze\u{a0}", "Breeze\u{a0}"),
            // Escaped keys and group names.
            (r"The\x6de=Escaped", "Escaped"),
        ] {
            assert_eq!(said(line).as_deref(), Some(read), "{line}");
        }
        assert_eq!(
            kconfig_value(
                &["[Colors\\x3aWindow]\nTheme=1\n"],
                "Colors:Window",
                "Theme"
            )
            .as_deref(),
            Some("1")
        );
        // A group below another is named apart from an escaped `][`.
        assert_eq!(kconfig_value(&["[A][B]\nTheme=1\n"], "A][B", "Theme"), None);
        assert_eq!(
            kconfig_value(&["[A\\x5d\\x5bB]\nTheme=1\n"], "A][B", "Theme").as_deref(),
            Some("1")
        );
        // A value escaped to nothing says nothing, as an empty one.
        assert_eq!(
            kconfig_value(
                &["[Icons]\nTheme=\\\n", "[Icons]\nTheme=breeze\n"],
                "Icons",
                "Theme"
            )
            .as_deref(),
            Some("breeze")
        );

        let environment = |name: &str| match name {
            "THEME" => Some("Papirus".to_owned()),
            "DOLLAR" => Some("$THEME".to_owned()),
            _ => None,
        };
        let expanded =
            |cascade: &[&str]| kconfig_value_with(cascade, "Icons", "Theme", &environment);
        for (line, read) in [
            ("Theme[$e]=$THEME", "Papirus"),
            ("Theme[$e]=${THEME}-Dark", "Papirus-Dark"),
            ("Theme[$e]=$THEME_Dark", ""),
            ("Theme[$e]=${THEME", "Papirus"),
            ("Theme[$e]=$$THEME", "$THEME"),
            ("Theme[$e]=a$", "a$"),
            ("Theme[$e]=a$/b${}c", "a/bc"),
            // What a variable gives is not expanded again.
            ("Theme[$e]=$DOLLAR", "$THEME"),
            // No command is run: `$(…)` loses its `$` alone.
            ("Theme[$e]=$(echo Papirus)", "(echo Papirus)"),
            // Locked and expanded; escapes read before the expansion.
            ("Theme[$ie]=\\x24THEME", "Papirus"),
            // Not expanded without `[$e]`.
            ("Theme=$THEME", "$THEME"),
        ] {
            let read = (!read.is_empty()).then_some(read);
            let file = format!("[Icons]\n{line}\n");
            assert_eq!(expanded(&[file.as_str()]).as_deref(), read, "{line}");
        }
        // A value expanded to nothing leaves the earlier file's, as an
        // empty one does.
        assert_eq!(
            expanded(&["[Icons]\nTheme[$e]=$UNSET\n", "[Icons]\nTheme=breeze\n"]).as_deref(),
            Some("breeze")
        );
        // Qt's folders where their variables are not set: the user's.
        if std::env::var_os("QT_CONFIG_HOME").is_none() {
            assert_eq!(
                kconfig_variable("QT_CONFIG_HOME").map(PathBuf::from),
                crate::xdg::config_home()
            );
        }
        assert_eq!(kconfig_variable("SKWIR_TEST_KCONFIG_UNSET"), None);
    }

    /// The files `KConfig` reads as `kdeglobals`, in its order: the
    /// user's `kdeglobals` and `system.kdeglobals`, the system folders'
    /// `kdeglobals`, then their `system.kdeglobals`, `/etc/kde5rc` last,
    /// so an administrator's lock in either holds over the user's word.
    #[test]
    fn kdeglobals_are_found_where_kconfig_looks() {
        let dir = tempfile::tempdir().unwrap();
        let at = |relative: &str| dir.path().join(relative);
        let write = |relative: &str, text: &str| {
            std::fs::create_dir_all(at(relative).parent().unwrap()).unwrap();
            std::fs::write(at(relative), text).unwrap();
        };
        let (home, dirs) = (at("home"), [at("look"), at("system")]);
        let kderc = Some(at("kde5rc"));
        let cascade = || kdeglobals_cascade_in(Some(&home), &dirs, kderc.clone());
        assert!(cascade().is_empty());
        let files = [
            "home/kdeglobals",
            "home/system.kdeglobals",
            "look/kdeglobals",
            "system/kdeglobals",
            "look/system.kdeglobals",
            "system/system.kdeglobals",
            "kde5rc",
        ];
        for file in files {
            write(file, &format!("[Recent]\nUse={file}\n"));
        }
        let said: Vec<String> = files
            .iter()
            .map(|f| format!("[Recent]\nUse={f}\n"))
            .collect();
        assert_eq!(cascade(), said);
        assert_eq!(
            kconfig_value(&cascade(), "Recent", "Use").as_deref(),
            Some("home/kdeglobals")
        );
        // An administrator's lock, in a system's `system.kdeglobals` or
        // in `/etc/kde5rc`, holds over every file before it.
        write("system/system.kdeglobals", "[Recent][$i]\nUse=locked\n");
        assert_eq!(
            kconfig_value(&cascade(), "Recent", "Use").as_deref(),
            Some("locked")
        );
        write("kde5rc", "[$i]\n[Recent]\nUse=kiosk\n");
        assert_eq!(
            kconfig_value(&cascade(), "Recent", "Use").as_deref(),
            Some("kiosk")
        );
        // `KDE_SKIP_KDERC`: no `/etc/kde5rc`.
        assert_eq!(
            kdeglobals_cascade_in(Some(&home), &dirs, None).len(),
            files.len() - 1
        );
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
