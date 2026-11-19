//! The desktop portal's choice of back ends (ADR-141, 10): which ones
//! are installed for the file chooser (their `.portal` files), which one
//! its configuration names, and the user's configuration written to name
//! another, its other lines kept.
//!
//! Found as xdg-desktop-portal 1.22 finds them. Its `.portal` files: in
//! `XDG_DESKTOP_PORTAL_DIR` alone where that is set, else in the data
//! folders' `xdg-desktop-portal/portals` (the user's first). Its
//! configuration: a file a folder, `<desktop>-portals.conf` for each of
//! `XDG_CURRENT_DESKTOP`'s desktops (lower case) before `portals.conf`,
//! one with a `[preferred]` group alone, in the user's configuration
//! folder, the system's, `/etc`'s, then the data folders'. An interface
//! goes to the first file naming a back end installed for it, its own key
//! before its `default`; a portal before 1.22 read the first file alone.
//! The portal reads all this once, at its start: a choice applies when it
//! starts again, which skwir never does (the session's service).
//!
//! Linux's: elsewhere the system's dialogs cannot be replaced (ADR-141, 11).

use std::path::{Path, PathBuf};

/// The interface the row chooses a back end for.
pub const FILE_CHOOSER: &str = "org.freedesktop.impl.portal.FileChooser";
/// The configuration's group of choices.
const PREFERRED: &str = "preferred";
/// The most a configuration or `.portal` file is read of.
const LIMIT: u64 = 256 * 1024;

/// A back end installed, by the name its `.portal` file gives it (`kde`
/// for `kde.portal`), which the configuration names.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BackEnd {
    pub name: String,
    pub interfaces: Vec<String>,
}

/// What the portal would choose from, and chooses.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Portals {
    /// The back ends installed that answer the file chooser, by name.
    pub choosers: Vec<String>,
    /// The one the configuration names for it, if any (else the portal
    /// falls back on the desktops' `UseIn` keys, then GTK's).
    pub chooser: Option<String>,
    /// The user's own configuration file, where there is one.
    pub user_file: Option<PathBuf>,
}

/// Where the portal looks, as read from the environment: kept apart so
/// that tests give their own.
#[derive(Clone, Debug, Default)]
pub struct Places {
    /// The folders of `.portal` files, in order.
    pub portal_dirs: Vec<PathBuf>,
    /// The configuration's folders, in order, the user's first.
    pub config_dirs: Vec<PathBuf>,
    /// The user's configuration folder, which a choice writes in.
    pub user_dir: Option<PathBuf>,
    /// `XDG_CURRENT_DESKTOP`'s desktops, lower case.
    pub desktops: Vec<String>,
}

impl Places {
    /// Where the portal of this session looks.
    pub fn here() -> Places {
        const SUBDIR: &str = "xdg-desktop-portal";
        let desktops: Vec<String> = crate::apps::current_desktops()
            .iter()
            .map(|desktop| desktop.to_ascii_lowercase())
            .collect();
        if let Some(dir) = std::env::var_os("XDG_DESKTOP_PORTAL_DIR").filter(|d| !d.is_empty()) {
            // The portal's tests' setting: that folder alone, for both.
            let dir = PathBuf::from(dir);
            return Places {
                portal_dirs: vec![dir.clone()],
                config_dirs: vec![dir.clone()],
                user_dir: Some(dir),
                desktops,
            };
        }
        let data = skwir_core::xdg::data_search();
        let mut portal_dirs: Vec<PathBuf> = data
            .iter()
            .map(|d| d.join(SUBDIR).join("portals"))
            .collect();
        let user_dir = skwir_core::xdg::config_home().map(|c| c.join(SUBDIR));
        let mut config_dirs: Vec<PathBuf> = user_dir.iter().cloned().collect();
        config_dirs.extend(
            skwir_core::xdg::config_dirs()
                .iter()
                .map(|d| d.join(SUBDIR)),
        );
        config_dirs.push(PathBuf::from("/etc").join(SUBDIR));
        config_dirs.extend(data.iter().map(|d| d.join(SUBDIR)));
        // The portal's own data folder, where a distribution installs it
        // outside the data folders named.
        let usr = PathBuf::from("/usr/share").join(SUBDIR);
        if !config_dirs.contains(&usr) {
            portal_dirs.push(usr.join("portals"));
            config_dirs.push(usr);
        }
        Places {
            portal_dirs,
            config_dirs,
            user_dir,
            desktops,
        }
    }

    /// The configuration file the portal reads in `dir`, if any: the
    /// first desktop's that has choices, else `portals.conf`'s. Reads it.
    fn config_in(&self, dir: &Path) -> Option<(PathBuf, Config)> {
        self.desktops
            .iter()
            .map(|desktop| format!("{desktop}-portals.conf"))
            .chain(std::iter::once("portals.conf".to_owned()))
            .find_map(|name| {
                let path = dir.join(name);
                let config = Config::read(&path)?;
                config.has_choices().then_some((path, config))
            })
    }

    /// The file a choice is written in: the user's own the portal reads,
    /// else the one it would read first there, made.
    fn user_file(&self) -> Option<PathBuf> {
        let dir = self.user_dir.as_ref()?;
        if let Some((path, _)) = self.config_in(dir) {
            return Some(path);
        }
        Some(dir.join(match self.desktops.first() {
            Some(desktop) => format!("{desktop}-portals.conf"),
            None => "portals.conf".to_owned(),
        }))
    }
}

/// What the portal would choose from, and chooses, read from `places`.
/// Reads the disk: on a worker.
pub fn read(places: &Places) -> Portals {
    let installed = installed(places);
    let answers = |name: &str| {
        installed
            .iter()
            .any(|b| b.name == name && b.interfaces.iter().any(|i| i == FILE_CHOOSER))
    };
    let configs: Vec<(PathBuf, Config)> = places
        .config_dirs
        .iter()
        .filter_map(|dir| places.config_in(dir))
        .collect();
    // As the portal reads them: the first file naming one installed, its
    // own key before its default.
    let chooser = configs.iter().find_map(|(_, config)| {
        [FILE_CHOOSER, "default"]
            .iter()
            .find_map(|key| config.names(key).into_iter().find(|name| answers(name)))
    });
    let user_file = places
        .user_dir
        .as_ref()
        .and_then(|dir| places.config_in(dir))
        .map(|(path, _)| path);
    let mut choosers: Vec<String> = installed
        .iter()
        .filter(|b| b.interfaces.iter().any(|i| i == FILE_CHOOSER))
        .map(|b| b.name.clone())
        .collect();
    choosers.sort();
    choosers.dedup();
    Portals {
        choosers,
        chooser,
        user_file,
    }
}

/// The back ends installed: each name once, the first folder's.
fn installed(places: &Places) -> Vec<BackEnd> {
    let mut found: Vec<BackEnd> = Vec::new();
    for dir in &places.portal_dirs {
        let Ok(entries) = std::fs::read_dir(dir) else {
            continue;
        };
        let mut names: Vec<PathBuf> = entries.flatten().map(|e| e.path()).collect();
        names.sort();
        for path in names {
            let Some(name) = path
                .file_name()
                .and_then(|n| n.to_str())
                .and_then(|n| n.strip_suffix(".portal"))
                .map(str::to_owned)
            else {
                continue;
            };
            if found.iter().any(|b| b.name == name) {
                continue;
            }
            let Some(text) = read_text(&path) else {
                continue;
            };
            let interfaces = keys_of(&text, "portal")
                .into_iter()
                .find(|(key, _)| key == "Interfaces")
                .map(|(_, value)| crate::keyfile::list(&value))
                .unwrap_or_default();
            found.push(BackEnd { name, interfaces });
        }
    }
    found
}

/// What a choice changed: the user's file as it was (`None`: there was
/// none), to put back.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Before {
    pub file: PathBuf,
    pub text: Option<String>,
}

/// Name `chooser` for the file chooser in the user's configuration (empty:
/// none, its key taken out), its other lines kept. Where the user has no
/// file, the one the portal reads first is copied into it before: a
/// portal before 1.22 reads that file alone, and a new one would drop the
/// system's other choices. What it was, for Undo; `None` when it already
/// said so. Writes the disk: on a worker.
///
/// # Errors
/// When the folder or the file cannot be written.
pub fn choose(places: &Places, chooser: &str) -> std::io::Result<Option<Before>> {
    let Some(file) = places.user_file() else {
        return Err(std::io::Error::other(skwir_core::words!(
            portals_no_config_folder = "There is no configuration folder to write the choice in",
            note = "An error choosing what answers other programs' Open and Save dialogs: the user has no configuration folder (no XDG_CONFIG_HOME, no home)."
        )));
    };
    let before = read_text(&file);
    let base = match &before {
        Some(text) => text.clone(),
        None => places
            .config_dirs
            .iter()
            .filter(|dir| Some(*dir) != places.user_dir.as_ref())
            .find_map(|dir| places.config_in(dir))
            .and_then(|(path, _)| read_text(&path))
            .unwrap_or_default(),
    };
    let text = with_key(
        &base,
        PREFERRED,
        FILE_CHOOSER,
        Some(chooser).filter(|c| !c.is_empty()),
    );
    if before.as_deref() == Some(text.as_str()) {
        return Ok(None);
    }
    write(&file, &text)?;
    Ok(Some(Before { file, text: before }))
}

/// Put back what `choose` changed: the file as it was, or none.
///
/// # Errors
/// When the file cannot be written or taken away.
pub fn put_back(before: &Before) -> std::io::Result<()> {
    match &before.text {
        Some(text) => write(&before.file, text),
        None => match std::fs::remove_file(&before.file) {
            Err(error) if error.kind() != std::io::ErrorKind::NotFound => Err(error),
            _ => Ok(()),
        },
    }
}

fn write(file: &Path, text: &str) -> std::io::Result<()> {
    let dir = file.parent().unwrap_or(Path::new("."));
    std::fs::create_dir_all(dir)?;
    let mut staged = tempfile::NamedTempFile::new_in(dir)?;
    std::io::Write::write_all(&mut staged, text.as_bytes())?;
    crate::file::take_place(staged, file)
}

fn read_text(path: &Path) -> Option<String> {
    let bytes = crate::file::read_regular(path, LIMIT).ok()?;
    String::from_utf8(bytes).ok()
}

/// A configuration file, as the portal reads it: its `[preferred]` keys.
#[derive(Debug)]
struct Config {
    preferred: Vec<(String, String)>,
}

impl Config {
    fn read(path: &Path) -> Option<Config> {
        let text = read_text(path)?;
        Some(Config {
            preferred: keys_of(&text, PREFERRED),
        })
    }

    /// A file without choices is passed over, as the portal does.
    fn has_choices(&self) -> bool {
        !self.preferred.is_empty()
    }

    /// The back ends `key` names, in order.
    fn names(&self, key: &str) -> Vec<String> {
        self.preferred
            .iter()
            .find(|(k, _)| k == key)
            .map(|(_, value)| crate::keyfile::list(value))
            .unwrap_or_default()
    }
}

/// The keys of `group` in a key file, in order.
fn keys_of(text: &str, group: &str) -> Vec<(String, String)> {
    crate::keyfile::entries(text)
        .filter(|(g, _, _)| *g == group)
        .map(|(_, key, value)| (key.to_owned(), value.trim_end().to_owned()))
        .collect()
}

/// `text` with `key` of `group` set to `value` (`None`: taken out), every
/// other line as it was; the group made at the end where there is none,
/// the key at the end of the group where it has none.
fn with_key(text: &str, group: &str, key: &str, value: Option<&str>) -> String {
    let mut out: Vec<String> = Vec::new();
    let (mut inside, mut found_group, mut done) = (false, false, false);
    let line_for = |value: &str| format!("{key}={value}");
    for line in text.lines() {
        let trimmed = line.trim();
        if let Some(name) = trimmed.strip_prefix('[').and_then(|l| l.strip_suffix(']')) {
            if inside && !done {
                // The group ends without the key: it goes at its end,
                // before the blank lines that part it from the next.
                if let Some(value) = value {
                    let at = out
                        .iter()
                        .rposition(|l| !l.trim().is_empty())
                        .map_or(0, |i| i + 1);
                    out.insert(at, line_for(value));
                }
                done = true;
            }
            inside = name == group;
            found_group |= inside;
            out.push(line.to_owned());
            continue;
        }
        if inside
            && !trimmed.starts_with('#')
            && trimmed
                .split_once('=')
                .is_some_and(|(k, _)| k.trim() == key)
        {
            if !done && let Some(value) = value {
                out.push(line_for(value));
            }
            done = true;
            continue;
        }
        out.push(line.to_owned());
    }
    if !done && let Some(value) = value {
        if inside {
            let at = out
                .iter()
                .rposition(|l| !l.trim().is_empty())
                .map_or(0, |i| i + 1);
            out.insert(at, line_for(value));
        } else {
            if !found_group {
                if out.last().is_some_and(|l| !l.trim().is_empty()) {
                    out.push(String::new());
                }
                out.push(format!("[{group}]"));
            }
            out.push(line_for(value));
        }
    }
    let mut text = out.join("\n");
    text.push('\n');
    text
}

#[cfg(test)]
mod tests {
    use super::*;

    fn portal(dir: &Path, name: &str, interfaces: &str) {
        std::fs::create_dir_all(dir).unwrap();
        std::fs::write(
            dir.join(format!("{name}.portal")),
            format!("[portal]\nDBusName=org.freedesktop.impl.portal.desktop.{name}\nInterfaces={interfaces}\n"),
        )
        .unwrap();
    }

    fn places(root: &Path, desktops: &[&str]) -> Places {
        Places {
            portal_dirs: vec![root.join("data/portals"), root.join("sys/portals")],
            config_dirs: vec![root.join("config"), root.join("etc")],
            user_dir: Some(root.join("config")),
            desktops: desktops.iter().map(|d| (*d).to_owned()).collect(),
        }
    }

    /// The back ends listed are those installed for the file chooser; the
    /// one chosen is the first file's naming one of them, its own key
    /// before its default, a desktop's file before `portals.conf`, one
    /// without choices passed over.
    #[test]
    fn the_choice_is_read_as_the_portal_reads_it() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        portal(
            &root.join("sys/portals"),
            "kde",
            "org.freedesktop.impl.portal.FileChooser;org.freedesktop.impl.portal.Print;",
        );
        portal(
            &root.join("sys/portals"),
            "gtk",
            "org.freedesktop.impl.portal.FileChooser;",
        );
        portal(
            &root.join("sys/portals"),
            "hyprland",
            "org.freedesktop.impl.portal.Screenshot;",
        );
        portal(&root.join("data/portals"), "skwir", FILE_CHOOSER);
        let p = places(root, &["hyprland"]);
        let read = read(&p);
        assert_eq!(read.choosers, ["gtk", "kde", "skwir"]);
        assert_eq!(read.chooser, None);

        std::fs::create_dir_all(root.join("etc")).unwrap();
        std::fs::write(root.join("etc/portals.conf"), "[preferred]\ndefault=gtk\n").unwrap();
        assert_eq!(super::read(&p).chooser.as_deref(), Some("gtk"));
        std::fs::write(
            root.join("etc/hyprland-portals.conf"),
            "[preferred]\ndefault=hyprland;kde\norg.freedesktop.impl.portal.FileChooser=nautilus;kde\n",
        )
        .unwrap();
        assert_eq!(
            super::read(&p).chooser.as_deref(),
            Some("kde"),
            "one not installed is passed over"
        );
        std::fs::create_dir_all(root.join("config")).unwrap();
        std::fs::write(root.join("config/portals.conf"), "# no choices\n").unwrap();
        assert_eq!(
            super::read(&p).chooser.as_deref(),
            Some("kde"),
            "a file without choices is passed over"
        );
        assert_eq!(super::read(&p).user_file, None);
    }

    /// A choice is written in the user's own file, its other lines and
    /// comments kept; where the user had none, the one the portal reads
    /// first is copied in before. Undo puts back the file as it was, or
    /// none; a choice already so writes nothing.
    #[test]
    fn a_choice_keeps_the_rest_of_the_configuration() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        portal(&root.join("sys/portals"), "kde", FILE_CHOOSER);
        portal(&root.join("data/portals"), "skwir", FILE_CHOOSER);
        std::fs::create_dir_all(root.join("etc")).unwrap();
        std::fs::write(
            root.join("etc/hyprland-portals.conf"),
            "[preferred]\ndefault=hyprland;kde\norg.freedesktop.impl.portal.Screenshot=hyprland\n",
        )
        .unwrap();
        let p = places(root, &["hyprland"]);
        let first = choose(&p, "skwir").unwrap().expect("changed");
        let user = root.join("config/hyprland-portals.conf");
        assert_eq!(
            first,
            Before {
                file: user.clone(),
                text: None
            }
        );
        assert_eq!(
            std::fs::read_to_string(&user).unwrap(),
            "[preferred]\ndefault=hyprland;kde\norg.freedesktop.impl.portal.Screenshot=hyprland\norg.freedesktop.impl.portal.FileChooser=skwir\n"
        );
        assert_eq!(read(&p).chooser.as_deref(), Some("skwir"));
        assert_eq!(choose(&p, "skwir").unwrap(), None, "already so");

        std::fs::write(
            &user,
            "# mine\n[preferred]\ndefault=hyprland;kde\norg.freedesktop.impl.portal.FileChooser=skwir\n\n[other]\nx=1\n",
        )
        .unwrap();
        let second = choose(&p, "kde").unwrap().expect("changed");
        assert_eq!(
            std::fs::read_to_string(&user).unwrap(),
            "# mine\n[preferred]\ndefault=hyprland;kde\norg.freedesktop.impl.portal.FileChooser=kde\n\n[other]\nx=1\n"
        );
        put_back(&second).unwrap();
        assert!(
            std::fs::read_to_string(&user)
                .unwrap()
                .contains("FileChooser=skwir")
        );
        put_back(&first).unwrap();
        assert!(!user.exists(), "the user had none");
    }

    /// Taking the key out leaves the rest; a group without it gets it at
    /// its end, before what parts it from the next.
    #[test]
    fn a_key_is_set_in_its_group() {
        assert_eq!(
            with_key(
                "[preferred]\ndefault=kde\n\n[x]\ny=1\n",
                "preferred",
                FILE_CHOOSER,
                Some("skwir")
            ),
            "[preferred]\ndefault=kde\norg.freedesktop.impl.portal.FileChooser=skwir\n\n[x]\ny=1\n"
        );
        assert_eq!(
            with_key(
                "[preferred]\ndefault=kde\norg.freedesktop.impl.portal.FileChooser=gtk\n",
                "preferred",
                FILE_CHOOSER,
                None
            ),
            "[preferred]\ndefault=kde\n"
        );
        assert_eq!(
            with_key("", "preferred", FILE_CHOOSER, Some("skwir")),
            "[preferred]\norg.freedesktop.impl.portal.FileChooser=skwir\n"
        );
    }
}
