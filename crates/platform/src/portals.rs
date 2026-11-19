//! The desktop portal's choice of back ends (ADR-141, 10): which ones
//! are installed for the file chooser (their `.portal` files), which one
//! its configuration names, and the user's configuration written to name
//! another, its other lines kept.
//!
//! Found and read as xdg-desktop-portal 1.22.1 does (its
//! `xdp-portal-config.c`). Its `.portal` files: in `XDG_DESKTOP_PORTAL_DIR`
//! alone where that is set, else in the data folders'
//! `xdg-desktop-portal/portals` (the user's first), a name's first file
//! taken. Its configuration: a file a folder, `<desktop>-portals.conf` for
//! each of `XDG_CURRENT_DESKTOP`'s desktops (lower case) before
//! `portals.conf`, the first with a `[preferred]` group (keys or none), in
//! the user's configuration folder, the system's, `/etc`'s, then the data
//! folders'. Each key as `GKeyFile` reads it, a key written twice its last
//! value. An interface goes to the first file whose own key, else its
//! `default`, names a back end installed for it: `none` there (its own key's,
//! else the default's) gives it none, the cascade ending; `*` the first
//! installed (those whose `UseIn` names a desktop first, then by name). A
//! portal before 1.22 read the first file alone. The portal reads all this
//! once, at its start: a choice applies when it starts again, which skwir
//! never does (the session's service); what changed since the portal that
//! runs started is said pending (its process's start, from the bus and
//! `/proc`). A configuration of the user's that is a link (its file or its
//! folder: home-manager's, a dotfiles folder's) is another program's to
//! keep: never written, where it leads said instead (the user's choice,
//! 2026-11-19). Where the user has no file, a choice copies the one the
//! portal reads first only for a portal before 1.22, which takes it whole:
//! the version of the one its next start runs, asked of its program.
//!
//! Linux's: elsewhere the system's dialogs cannot be replaced (ADR-141, 11).

use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

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
    /// The desktops it offers itself for (`UseIn`), which order `*`.
    pub use_in: Vec<String>,
}

impl BackEnd {
    fn chooses(&self) -> bool {
        self.interfaces.iter().any(|i| i == FILE_CHOOSER)
    }
}

/// What the configuration gives the file chooser.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Named {
    /// This back end, installed.
    BackEnd(String),
    /// `none`: no back end, so no dialog from the portal.
    Refused,
    /// No back end it names is installed: the portal falls back on the
    /// back ends' `UseIn` keys, then GTK's.
    Unnamed,
}

/// What the portal would choose from, and chooses.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Portals {
    /// The back ends installed that answer the file chooser, by name.
    pub choosers: Vec<String>,
    /// What the configuration gives it.
    pub chooser: Named,
    /// The user's own configuration file, where there is one.
    pub user_file: Option<PathBuf>,
    /// The configuration changed since the running portal read it (a
    /// file or a folder of it written after the portal started): what
    /// it names applies at the portal's next start.
    pub pending: bool,
    /// The user's configuration is a link (its file or its folder), kept
    /// by another program: where it leads. Nothing is written there.
    pub linked: Option<PathBuf>,
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
    /// When the portal that runs started, and read its configuration
    /// (`None`: none runs, or it is not known).
    pub started: Option<SystemTime>,
    /// The portal's program its next start runs (its bus service file's
    /// `Exec`), whose version decides whether a choice copies the
    /// configuration in effect (`None`: not found, so it does).
    pub program: Option<PathBuf>,
}

impl Places {
    /// Where the portal of this session looks, and when it started (asked
    /// of the bus, read in `/proc`): on a worker.
    pub fn here() -> Places {
        const SUBDIR: &str = "xdg-desktop-portal";
        let desktops: Vec<String> = crate::apps::current_desktops()
            .iter()
            .map(|desktop| desktop.to_ascii_lowercase())
            .collect();
        let started = portal_started();
        let data = skwir_core::xdg::data_search();
        let program = installed_portal(&data);
        if let Some(dir) = std::env::var_os("XDG_DESKTOP_PORTAL_DIR").filter(|d| !d.is_empty()) {
            // The portal's tests' setting: that folder alone, for both.
            let dir = PathBuf::from(dir);
            return Places {
                portal_dirs: vec![dir.clone()],
                config_dirs: vec![dir.clone()],
                user_dir: Some(dir),
                desktops,
                started,
                program,
            };
        }
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
            started,
            program,
        }
    }

    /// The configuration file the portal reads in `dir`, if any: the
    /// first desktop's with a `[preferred]` group, else `portals.conf`'s.
    /// One that cannot be read is passed over, as the portal does. Reads
    /// it.
    fn config_in(&self, dir: &Path) -> Option<(PathBuf, Config)> {
        self.desktops
            .iter()
            .map(|desktop| format!("{desktop}-portals.conf"))
            .chain(std::iter::once("portals.conf".to_owned()))
            .find_map(|name| {
                let path = dir.join(name);
                Config::read(&path).map(|config| (path, config))
            })
    }

    /// Where the user's configuration leads when its folder or its file is
    /// a link (home-manager's, a dotfiles folder's): another program keeps
    /// it, and skwir leaves it as it is (the user's choice, 2026-11-19).
    fn linked(&self) -> Option<PathBuf> {
        let dir = self.user_dir.as_ref()?;
        [Some(dir.clone()), self.user_file()]
            .into_iter()
            .flatten()
            .find_map(|path| link_target(&path))
    }

    /// Whether the portal its next start runs reads its files in a cascade
    /// (1.22 and after), so that a choice of the user's needs only its own
    /// line; a version not known is taken for one before. Runs the
    /// portal's program (`--version`): on a worker, as a choice is written.
    fn cascades(&self) -> bool {
        self.program
            .as_deref()
            .and_then(|program| crate::desktop_settings::printed(program, &["--version"]))
            .and_then(|printed| version_of(&printed))
            .is_some_and(|version| version >= (1, 22))
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
    let found: Vec<(PathBuf, Config)> = places
        .config_dirs
        .iter()
        .filter_map(|dir| places.config_in(dir))
        .collect();
    let pending = places.started.is_some_and(|started| {
        changed_since(
            started,
            places
                .config_dirs
                .iter()
                .chain(found.iter().map(|(path, _)| path)),
        )
    });
    let configs: Vec<Config> = found.into_iter().map(|(_, config)| config).collect();
    let chooser = named(&configs, &installed, &places.desktops);
    let user_file = places
        .user_dir
        .as_ref()
        .and_then(|dir| places.config_in(dir))
        .map(|(path, _)| path);
    let mut choosers: Vec<String> = installed
        .iter()
        .filter(|b| b.chooses())
        .map(|b| b.name.clone())
        .collect();
    choosers.sort();
    choosers.dedup();
    Portals {
        choosers,
        chooser,
        user_file,
        pending,
        linked: places.linked(),
    }
}

/// Where `path` leads, when it is a link: its target, read against its
/// folder where it is relative.
fn link_target(path: &Path) -> Option<PathBuf> {
    let meta = std::fs::symlink_metadata(path).ok()?;
    if !meta.file_type().is_symlink() {
        return None;
    }
    let target = std::fs::read_link(path).ok()?;
    Some(match path.parent() {
        Some(dir) if target.is_relative() => dir.join(target),
        _ => target,
    })
}

/// The program the portal's next start runs: its bus service file's
/// `Exec` (`org.freedesktop.portal.Desktop.service`), the first the bus
/// finds in the data folders (the user's first), then `/usr/share`'s.
fn installed_portal(data: &[PathBuf]) -> Option<PathBuf> {
    data.iter()
        .cloned()
        .chain(std::iter::once(PathBuf::from("/usr/share")))
        .find_map(|dir| {
            let text =
                read_text(&dir.join("dbus-1/services/org.freedesktop.portal.Desktop.service"))?;
            let exec = keys_of(&text, "D-BUS Service")
                .into_iter()
                .rev()
                .find(|(key, _)| key == "Exec")?
                .1;
            let (program, _) = crate::keyfile::exec_words(&exec).into_iter().next()?;
            Some(PathBuf::from(program))
        })
}

/// A portal's version as it prints it (`xdg-desktop-portal 1.22.1`): its
/// major and minor numbers.
fn version_of(printed: &str) -> Option<(u32, u32)> {
    let version = printed.split_whitespace().last()?;
    let mut numbers = version.split('.');
    Some((numbers.next()?.parse().ok()?, numbers.next()?.parse().ok()?))
}

/// Why a choice is not written: the user's configuration is a link, which
/// another program keeps.
fn linked_error(target: &Path) -> std::io::Error {
    std::io::Error::other(skwir_core::words!(
        portals_linked = "it is a link to { $target }, which another program keeps: choose there",
        target = target,
        note = "Why a choice of what answers other programs' Open and Save dialogs was not written: the user's portal configuration (its file or its folder) is a link, kept by a program such as home-manager or a dotfiles folder; target: where the link leads. Begins in lower case: said after other words."
    ))
}

/// Whether any of `paths` (the configuration's folders, which a file made,
/// replaced or taken away there changes, and its files, which an edit in
/// place changes) was written after `started`: two seconds after, the
/// start being known to the second.
fn changed_since<'a>(started: SystemTime, paths: impl Iterator<Item = &'a PathBuf>) -> bool {
    let after = started + Duration::from_secs(2);
    paths
        .filter_map(|path| std::fs::metadata(path).and_then(|m| m.modified()).ok())
        .any(|modified| modified > after)
}

/// When the desktop portal that runs started: its process's, asked of the
/// bus and read in `/proc`. On a worker.
fn portal_started() -> Option<SystemTime> {
    let bus = crate::bus::session().ok()?;
    let dbus = zbus::blocking::fdo::DBusProxy::new(&bus).ok()?;
    let name = zbus::names::BusName::try_from("org.freedesktop.portal.Desktop").ok()?;
    started_at(dbus.get_connection_unix_process_id(name).ok()?)
}

/// When the process `pid` started: its start in clock ticks after boot
/// (`/proc/<pid>/stat`'s 22nd field), after the boot's time (`/proc/stat`'s
/// `btime`, in seconds).
fn started_at(pid: u32) -> Option<SystemTime> {
    let stat = std::fs::read_to_string(format!("/proc/{pid}/stat")).ok()?;
    // Read after the name, which may hold spaces and parentheses: the
    // third field (its state) first.
    let ticks: u64 = stat[stat.rfind(')')? + 1..]
        .split_whitespace()
        .nth(22 - 3)?
        .parse()
        .ok()?;
    let boot: u64 = std::fs::read_to_string("/proc/stat")
        .ok()?
        .lines()
        .find_map(|line| line.strip_prefix("btime "))?
        .trim()
        .parse()
        .ok()?;
    let hz = rustix::param::clock_ticks_per_second().max(1);
    Some(
        SystemTime::UNIX_EPOCH
            + Duration::from_secs(boot + ticks / hz)
            + Duration::from_nanos((ticks % hz) * 1_000_000_000 / hz),
    )
}

/// What `configs` give the file chooser, as the portal goes through them
/// (`xdp_portal_config_find`): in each, `none` ends it with none; else its
/// own key's names, then its default's, the first installed for it taken.
fn named(configs: &[Config], installed: &[BackEnd], desktops: &[String]) -> Named {
    let refuses = |names: &Option<Vec<String>>| {
        names
            .as_ref()
            .is_some_and(|names| names.iter().any(|n| n == "none"))
    };
    for config in configs {
        let own = config.names(FILE_CHOOSER);
        let default = config.names("default");
        // The default's `none` counts only where the file has no key of
        // the interface's own.
        if refuses(&own) || (own.is_none() && refuses(&default)) {
            return Named::Refused;
        }
        for names in [own, default].into_iter().flatten() {
            if let Some(name) = first_installed(&names, installed, desktops) {
                return Named::BackEnd(name);
            }
        }
    }
    Named::Unnamed
}

/// The first of `names` installed for the file chooser; `*` stands for the
/// first installed, and ends the list either way, as the portal reads it.
fn first_installed(names: &[String], installed: &[BackEnd], desktops: &[String]) -> Option<String> {
    for name in names {
        if name == "*" {
            return any_installed(installed, desktops);
        }
        if installed.iter().any(|b| b.name == *name && b.chooses()) {
            return Some(name.clone());
        }
    }
    None
}

/// What `*` names: the back ends for the file chooser in the portal's
/// order, the first desktop one of two offers itself for first
/// (`UseIn`, case ignored), then by name.
fn any_installed(installed: &[BackEnd], desktops: &[String]) -> Option<String> {
    let uses =
        |b: &BackEnd, desktop: &str| b.use_in.iter().any(|d| d.eq_ignore_ascii_case(desktop));
    installed
        .iter()
        .filter(|b| b.chooses())
        .min_by(|a, b| {
            for desktop in desktops {
                match (uses(a, desktop), uses(b, desktop)) {
                    (true, false) => return std::cmp::Ordering::Less,
                    (false, true) => return std::cmp::Ordering::Greater,
                    (true, true) => break,
                    (false, false) => {}
                }
            }
            a.name.cmp(&b.name)
        })
        .map(|b| b.name.clone())
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
            let keys = keys_of(&text, "portal");
            // A key written twice is its last, as `GKeyFile` reads it.
            let list = |wanted: &str| {
                keys.iter()
                    .rev()
                    .find(|(key, _)| key == wanted)
                    .map(|(_, value)| crate::keyfile::list(value))
                    .unwrap_or_default()
            };
            found.push(BackEnd {
                interfaces: list("Interfaces"),
                use_in: list("UseIn"),
                name,
            });
        }
    }
    found
}

/// What a choice changed, for Undo: the user's file, as it was (`None`:
/// there was none), and what skwir wrote in it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Before {
    file: PathBuf,
    text: Option<String>,
    written: String,
}

/// Name `chooser` for the file chooser in the user's configuration (empty:
/// none, its key taken out), its other lines kept. Where the user has no
/// file, nothing is written for one already in effect, nor for the
/// desktop's choice (nothing of the user's to take out); for another, a
/// portal from 1.22 on is given that line alone, and for one before, or
/// one whose version is not known, the file the portal reads first is
/// copied in before: it takes that file whole, and a lone key would drop
/// the system's other choices (ADR-141, 10). What it was, for Undo; `None`
/// when it already said so. A file that is there and cannot be read is
/// left alone, the error said; a configuration that is a link (its file
/// or its folder) is never written: another program keeps it. Writes the
/// disk: on a worker.
///
/// # Errors
/// When the configuration is a link, the file cannot be read, or the
/// folder or the file written.
pub fn choose(places: &Places, chooser: &str) -> std::io::Result<Option<Before>> {
    let Some(file) = places.user_file() else {
        return Err(std::io::Error::other(skwir_core::words!(
            portals_no_config_folder = "There is no configuration folder to write the choice in",
            note = "An error choosing what answers other programs' Open and Save dialogs: the user has no configuration folder (no XDG_CONFIG_HOME, no home)."
        )));
    };
    if let Some(target) = places.linked() {
        return Err(linked_error(&target));
    }
    let before = text_at(&file)?;
    let named = Some(chooser).filter(|c| !c.is_empty());
    let text = match (&before, named) {
        (Some(text), _) => with_key(text, PREFERRED, FILE_CHOOSER, named),
        // Nothing of the user's to take out: the desktop chooses already.
        (None, None) => return Ok(None),
        (None, Some(name)) if read(places).chooser == Named::BackEnd(name.to_owned()) => {
            return Ok(None);
        }
        // A portal that cascades reads the system's files after this one.
        (None, Some(_)) if places.cascades() => with_key("", PREFERRED, FILE_CHOOSER, named),
        (None, Some(_)) => {
            let base = places
                .config_dirs
                .iter()
                .filter(|dir| Some(*dir) != places.user_dir.as_ref())
                .find_map(|dir| places.config_in(dir))
                .and_then(|(path, _)| read_text(&path))
                .unwrap_or_default();
            with_key(&base, PREFERRED, FILE_CHOOSER, named)
        }
    };
    if before.as_deref() == Some(text.as_str()) {
        return Ok(None);
    }
    write(&file, &text)?;
    Ok(Some(Before {
        file,
        text: before,
        written: text,
    }))
}

/// Put back what `choose` changed: the file as it was, or none; only while
/// it holds what skwir wrote, an edit made since being kept.
///
/// # Errors
/// When the file was changed since, cannot be read, written or taken
/// away.
pub fn put_back(before: &Before) -> std::io::Result<()> {
    // Made a link since: another program keeps it now.
    let dir = before.file.parent().unwrap_or(Path::new("."));
    if let Some(target) = link_target(&before.file).or_else(|| link_target(dir)) {
        return Err(linked_error(&target));
    }
    if text_at(&before.file)?.as_deref() != Some(before.written.as_str()) {
        return Err(std::io::Error::other(skwir_core::words!(
            portals_changed_since =
                "the file was changed since Skwir wrote it, and is kept as it is",
            note = "Why Undo of a choice of what answers other programs' Open and Save dialogs did nothing: the portal's configuration file was edited after the choice. Begins in lower case: said after other words."
        )));
    }
    match &before.text {
        Some(text) => write(&before.file, text),
        None => match std::fs::remove_file(&before.file) {
            Err(error) if error.kind() != std::io::ErrorKind::NotFound => Err(error),
            _ => Ok(()),
        },
    }
}

/// What `file` holds: `None` when nothing is there, an error when what is
/// there cannot be read as the portal's configuration (not allowed, not a
/// file, too large, not UTF-8).
fn text_at(file: &Path) -> std::io::Result<Option<String>> {
    match crate::file::read_regular(file, LIMIT) {
        Ok(bytes) => String::from_utf8(bytes).map(Some).map_err(|_| {
            std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                skwir_core::words!(
                    portals_not_utf8 = "it is not written in UTF-8",
                    note = "Why the desktop portal's configuration file was left alone: its text is not UTF-8, which the portal reads. Begins in lower case: said after other words."
                ),
            )
        }),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(error),
    }
}

/// `file` replaced whole by `text`, its mode kept (a new one is the
/// user's alone).
fn write(file: &Path, text: &str) -> std::io::Result<()> {
    let dir = file.parent().unwrap_or(Path::new("."));
    std::fs::create_dir_all(dir)?;
    let mut staged = tempfile::NamedTempFile::new_in(dir)?;
    std::io::Write::write_all(&mut staged, text.as_bytes())?;
    match std::fs::metadata(file) {
        Ok(meta) => std::fs::set_permissions(staged.path(), meta.permissions())?,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => return Err(error),
    }
    crate::file::take_place(staged, file)
}

fn read_text(path: &Path) -> Option<String> {
    text_at(path).ok().flatten()
}

/// A configuration file, as the portal reads it: its `[preferred]` keys.
#[derive(Debug)]
struct Config {
    preferred: Vec<(String, String)>,
}

impl Config {
    /// The file at `path`, if the portal takes it: one with a `[preferred]`
    /// group, even an empty one, as `GKeyFile` finds its keys; one without
    /// is passed over.
    fn read(path: &Path) -> Option<Config> {
        let text = read_text(path)?;
        let header = format!("[{PREFERRED}]");
        let grouped = text.lines().any(|line| line.trim() == header);
        grouped.then(|| Config {
            preferred: keys_of(&text, PREFERRED),
        })
    }

    /// The back ends `key` names, in order; `None` where it has no such
    /// key. A key written twice is its last, as `GKeyFile` reads it.
    fn names(&self, key: &str) -> Option<Vec<String>> {
        self.preferred
            .iter()
            .rev()
            .find(|(k, _)| k == key)
            .map(|(_, value)| crate::keyfile::list(value))
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
    use std::os::unix::fs::PermissionsExt;

    fn portal(dir: &Path, name: &str, interfaces: &str) {
        portal_for(dir, name, interfaces, "");
    }

    fn portal_for(dir: &Path, name: &str, interfaces: &str, use_in: &str) {
        std::fs::create_dir_all(dir).unwrap();
        std::fs::write(
            dir.join(format!("{name}.portal")),
            format!("[portal]\nDBusName=org.freedesktop.impl.portal.desktop.{name}\nInterfaces={interfaces}\nUseIn={use_in}\n"),
        )
        .unwrap();
    }

    fn places(root: &Path, desktops: &[&str]) -> Places {
        Places {
            portal_dirs: vec![root.join("data/portals"), root.join("sys/portals")],
            config_dirs: vec![root.join("config"), root.join("etc")],
            user_dir: Some(root.join("config")),
            desktops: desktops.iter().map(|d| (*d).to_owned()).collect(),
            started: None,
            program: None,
        }
    }

    fn named(name: &str) -> Named {
        Named::BackEnd(name.to_owned())
    }

    /// The back ends listed are those installed for the file chooser; the
    /// one chosen is the first file's naming one of them, its own key
    /// before its default, a desktop's file before `portals.conf`, one
    /// without a `[preferred]` group passed over.
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
        assert_eq!(read.chooser, Named::Unnamed);

        std::fs::create_dir_all(root.join("etc")).unwrap();
        std::fs::write(root.join("etc/portals.conf"), "[preferred]\ndefault=gtk\n").unwrap();
        assert_eq!(super::read(&p).chooser, named("gtk"));
        std::fs::write(
            root.join("etc/hyprland-portals.conf"),
            "[preferred]\ndefault=hyprland;kde\norg.freedesktop.impl.portal.FileChooser=nautilus;kde\n",
        )
        .unwrap();
        assert_eq!(
            super::read(&p).chooser,
            named("kde"),
            "one not installed is passed over"
        );
        std::fs::create_dir_all(root.join("config")).unwrap();
        std::fs::write(root.join("config/portals.conf"), "# no choices\n").unwrap();
        assert_eq!(
            super::read(&p).chooser,
            named("kde"),
            "a file without the group is passed over"
        );
        assert_eq!(super::read(&p).user_file, None);
    }

    /// `none` gives the file chooser no back end, ending the cascade (the
    /// default's only where the file has no key of its own); `*` the first
    /// installed, a desktop's own first; a key written twice is its last;
    /// a file with an empty `[preferred]` group is its folder's, as the
    /// portal takes it.
    #[test]
    fn none_any_and_repeats_are_read_as_the_portal_reads_them() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        portal(&root.join("sys/portals"), "gtk", FILE_CHOOSER);
        portal(&root.join("sys/portals"), "kde", FILE_CHOOSER);
        portal_for(&root.join("sys/portals"), "lxqt", FILE_CHOOSER, "LXQt;");
        let p = places(root, &["lxqt", "hyprland"]);
        std::fs::create_dir_all(root.join("etc")).unwrap();
        std::fs::create_dir_all(root.join("config")).unwrap();
        std::fs::write(root.join("etc/portals.conf"), "[preferred]\ndefault=kde\n").unwrap();
        let user = |text: &str| std::fs::write(root.join("config/portals.conf"), text).unwrap();

        user(&format!("[preferred]\n{FILE_CHOOSER}=none\ndefault=gtk\n"));
        assert_eq!(read(&p).chooser, Named::Refused, "its own none");
        user(&format!("[preferred]\n{FILE_CHOOSER}=gtk;none\n"));
        assert_eq!(read(&p).chooser, Named::Refused, "none anywhere in it");
        user("[preferred]\ndefault=none\n");
        assert_eq!(read(&p).chooser, Named::Refused, "the default's none");
        user(&format!(
            "[preferred]\n{FILE_CHOOSER}=nautilus\ndefault=none\n"
        ));
        assert_eq!(
            read(&p).chooser,
            named("kde"),
            "the default's none counts only without a key of its own"
        );
        user(&format!("[preferred]\n{FILE_CHOOSER}=nautilus;*;gtk\n"));
        assert_eq!(
            read(&p).chooser,
            named("lxqt"),
            "* is one offered for the desktop"
        );
        portal_for(&root.join("sys/portals"), "lxqt", FILE_CHOOSER, "");
        assert_eq!(read(&p).chooser, named("gtk"), "else by name");
        user(&format!(
            "[preferred]\n{FILE_CHOOSER}=gtk\n{FILE_CHOOSER}=kde\n"
        ));
        assert_eq!(
            read(&p).chooser,
            named("kde"),
            "a key written twice is its last"
        );
        user(&format!(
            "[preferred]\n{FILE_CHOOSER}=gtk\n\n[preferred]\n{FILE_CHOOSER}=lxqt\n"
        ));
        assert_eq!(
            read(&p).chooser,
            named("lxqt"),
            "so is one in a group written twice"
        );

        std::fs::write(root.join("config/lxqt-portals.conf"), "[preferred]\n").unwrap();
        assert_eq!(
            read(&p).chooser,
            named("kde"),
            "an empty group is the folder's file: its portals.conf is not read"
        );
        assert_eq!(
            read(&p).user_file,
            Some(root.join("config/lxqt-portals.conf"))
        );
    }

    /// A choice is written in the user's own file, its other lines and
    /// comments kept; where the user had none, the one the portal reads
    /// first is copied in before. Undo puts back the file as it was, or
    /// none, only while it holds what was written; a choice already so
    /// writes nothing.
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
        assert_eq!((&first.file, &first.text), (&user, &None));
        let copied = "[preferred]\ndefault=hyprland;kde\norg.freedesktop.impl.portal.Screenshot=hyprland\norg.freedesktop.impl.portal.FileChooser=skwir\n";
        assert_eq!(std::fs::read_to_string(&user).unwrap(), copied);
        assert_eq!(read(&p).chooser, named("skwir"));
        assert_eq!(choose(&p, "skwir").unwrap(), None, "already so");

        let mine = "# mine\n[preferred]\ndefault=hyprland;kde\norg.freedesktop.impl.portal.FileChooser=skwir\n\n[other]\nx=1\n";
        std::fs::write(&user, mine).unwrap();
        let second = choose(&p, "kde").unwrap().expect("changed");
        let chose_kde = "# mine\n[preferred]\ndefault=hyprland;kde\norg.freedesktop.impl.portal.FileChooser=kde\n\n[other]\nx=1\n";
        assert_eq!(std::fs::read_to_string(&user).unwrap(), chose_kde);
        put_back(&second).unwrap();
        assert_eq!(std::fs::read_to_string(&user).unwrap(), mine);
        let kept = put_back(&first).expect_err("edited since the first choice");
        assert!(kept.to_string().contains("changed since"), "{kept}");
        assert_eq!(std::fs::read_to_string(&user).unwrap(), mine, "kept");
        std::fs::write(&user, copied).unwrap();
        put_back(&first).unwrap();
        assert!(!user.exists(), "the user had none");
    }

    /// Where the user has no file, the desktop's choice and one already in
    /// effect write nothing; with a file of the user's, the desktop's
    /// choice takes out its key alone.
    #[test]
    fn nothing_is_written_for_what_is_so_already() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        portal(&root.join("sys/portals"), "kde", FILE_CHOOSER);
        portal(&root.join("sys/portals"), "gtk", FILE_CHOOSER);
        std::fs::create_dir_all(root.join("etc")).unwrap();
        std::fs::write(
            root.join("etc/hyprland-portals.conf"),
            format!("[preferred]\ndefault=gtk\n{FILE_CHOOSER}=kde\n"),
        )
        .unwrap();
        let p = places(root, &["hyprland"]);
        let user = root.join("config/hyprland-portals.conf");
        assert_eq!(choose(&p, "").unwrap(), None, "the desktop's choice");
        assert_eq!(choose(&p, "kde").unwrap(), None, "named already");
        assert!(!user.exists());

        std::fs::create_dir_all(root.join("config")).unwrap();
        std::fs::write(&user, format!("[preferred]\n{FILE_CHOOSER}=gtk\n# note\n")).unwrap();
        let undo = choose(&p, "").unwrap().expect("its key taken out");
        assert_eq!(
            std::fs::read_to_string(&user).unwrap(),
            "[preferred]\n# note\n"
        );
        assert_eq!(read(&p).chooser, named("kde"), "the desktop's again");
        put_back(&undo).unwrap();
        assert_eq!(read(&p).chooser, named("gtk"));
    }

    /// A file there that cannot be read as the configuration is left as it
    /// is and why said; one that is a link (its file, or its folder),
    /// another program's, is never written, where it leads said, even a
    /// link that leads nowhere yet.
    #[test]
    fn what_is_there_is_kept_and_a_link_left_to_its_keeper() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        portal(&root.join("sys/portals"), "kde", FILE_CHOOSER);
        portal(&root.join("sys/portals"), "skwir", FILE_CHOOSER);
        let p = places(root, &["hyprland"]);
        let user = root.join("config/hyprland-portals.conf");
        std::fs::create_dir_all(root.join("config")).unwrap();

        std::fs::write(&user, b"[preferred]\n# caf\xe9\ndefault=kde\n").unwrap();
        let refused = choose(&p, "skwir").expect_err("not UTF-8");
        assert!(refused.to_string().contains("UTF-8"), "{refused}");
        assert_eq!(
            std::fs::read(&user).unwrap(),
            b"[preferred]\n# caf\xe9\ndefault=kde\n"
        );
        std::fs::remove_file(&user).unwrap();
        std::fs::create_dir(&user).unwrap();
        choose(&p, "skwir").expect_err("a folder");
        assert!(user.is_dir());
        std::fs::remove_dir(&user).unwrap();

        // A link, as home-manager or a dotfiles folder makes: its target
        // left as it was, the link a link, where it leads said.
        let dotfiles = root.join("dotfiles");
        std::fs::create_dir_all(&dotfiles).unwrap();
        let target = dotfiles.join("portals.conf");
        std::fs::write(&target, "[preferred]\ndefault=kde\n").unwrap();
        std::fs::set_permissions(&target, std::fs::Permissions::from_mode(0o640)).unwrap();
        std::os::unix::fs::symlink("../dotfiles/portals.conf", &user).unwrap();
        let refused = choose(&p, "skwir").expect_err("a link");
        assert!(refused.to_string().contains("a link to"), "{refused}");
        assert!(refused.to_string().contains("dotfiles"), "{refused}");
        assert!(user.symlink_metadata().unwrap().file_type().is_symlink());
        assert_eq!(
            std::fs::read_to_string(&target).unwrap(),
            "[preferred]\ndefault=kde\n"
        );
        assert_eq!(
            read(&p).linked,
            Some(root.join("config/../dotfiles/portals.conf"))
        );
        assert_eq!(
            read(&p).chooser,
            named("kde"),
            "read through it all the same"
        );

        // One leading nowhere yet: nothing made.
        std::fs::remove_file(&target).unwrap();
        choose(&p, "skwir").expect_err("a link that leads nowhere");
        assert!(!target.exists());
        std::fs::remove_file(&user).unwrap();

        // The folder a link: nothing written in it either.
        std::fs::remove_dir(root.join("config")).unwrap();
        std::os::unix::fs::symlink("dotfiles", root.join("config")).unwrap();
        choose(&p, "skwir").expect_err("its folder a link");
        assert!(!dotfiles.join("hyprland-portals.conf").exists());
        assert_eq!(read(&p).linked, Some(root.join("dotfiles")));
    }

    /// Where the user has no file, a portal from 1.22 on (asked its
    /// version) is given the file chooser's line alone; one before, or one
    /// whose version is not known, a copy of the configuration in effect.
    #[test]
    fn a_portal_that_cascades_gets_one_line() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        portal(&root.join("sys/portals"), "kde", FILE_CHOOSER);
        portal(&root.join("sys/portals"), "skwir", FILE_CHOOSER);
        std::fs::create_dir_all(root.join("etc")).unwrap();
        std::fs::write(
            root.join("etc/hyprland-portals.conf"),
            format!("[preferred]\ndefault=hyprland;kde\n{FILE_CHOOSER}=kde\n"),
        )
        .unwrap();
        // The portal's program, standing in: it answers its version.
        let program = |version: &str| {
            let path = root.join(format!("xdg-desktop-portal-{version}"));
            std::fs::write(
                &path,
                format!("#!/bin/sh\necho 'xdg-desktop-portal {version}'\n"),
            )
            .unwrap();
            std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
            path
        };
        let user = root.join("config/hyprland-portals.conf");
        let mut p = places(root, &["hyprland"]);
        for (version, written) in [
            ("1.22.1", format!("[preferred]\n{FILE_CHOOSER}=skwir\n")),
            (
                "1.20.3",
                format!("[preferred]\ndefault=hyprland;kde\n{FILE_CHOOSER}=skwir\n"),
            ),
        ] {
            p.program = Some(program(version));
            let undo = choose(&p, "skwir").unwrap().expect("written");
            assert_eq!(
                std::fs::read_to_string(&user).unwrap(),
                written,
                "{version}"
            );
            put_back(&undo).unwrap();
        }
        p.program = Some(root.join("missing"));
        choose(&p, "skwir").unwrap().expect("written");
        assert!(
            std::fs::read_to_string(&user)
                .unwrap()
                .contains("default=hyprland;kde"),
            "not known: copied"
        );
        assert_eq!(version_of("xdg-desktop-portal 1.22.1\n"), Some((1, 22)));
        assert_eq!(version_of("1.18.4"), Some((1, 18)));
        assert_eq!(version_of("Unknown option --version"), None);
    }

    /// A configuration written after the portal started is pending: its
    /// file, or its folder for a file made or taken away; one read before
    /// is not. A process's start is read as the system dates it.
    #[test]
    fn a_change_after_the_portal_started_is_pending() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        portal(&root.join("sys/portals"), "kde", FILE_CHOOSER);
        std::fs::create_dir_all(root.join("etc")).unwrap();
        std::fs::write(root.join("etc/portals.conf"), "[preferred]\ndefault=kde\n").unwrap();
        let mut p = places(root, &["hyprland"]);
        assert!(!read(&p).pending, "no portal runs");
        p.started = Some(SystemTime::now() + Duration::from_secs(60));
        assert!(!read(&p).pending, "read as it started");
        p.started = Some(SystemTime::now() - Duration::from_secs(60));
        assert!(read(&p).pending, "written since");

        let now = SystemTime::now();
        let started = started_at(std::process::id()).expect("this process's start");
        assert!(started <= now && now.duration_since(started).unwrap() < Duration::from_secs(3600));
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
