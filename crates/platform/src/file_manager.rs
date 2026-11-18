//! Showing folders and files for other programs (ADR-140). While skwir
//! is the default the desktop names for folders, the program that takes
//! launches over answers `org.freedesktop.FileManager1` ("Show in
//! folder": [`crate::handover::Answering`]) and keeps the file that has
//! the session bus start skwir for it when none runs; when it is not, it
//! answers nothing and takes its file back. The choice is the desktop's
//! own: skwir keeps none of its own.
//!
//! Linux alone: elsewhere [`follow`] follows nothing (ADR-140, 7).

use std::collections::{HashMap, HashSet};
use std::path::PathBuf;

use crate::handover::Opening;

/// The type folders are, whose default is the choice.
pub const FOLDERS: &str = "inode/directory";

/// The argument the bus starts skwir with: the program starts without a
/// window and opens one for the request (ADR-140, 5).
pub const STARTED_BY_BUS: &str = "--file-manager";

/// What a request asks shown.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Shown {
    /// `ShowFolders`: each folder opened.
    Folders,
    /// `ShowItems`: the folder of each item opened, its items selected.
    Items,
    /// `ShowItemProperties`: as [`Shown::Items`], Properties shown.
    Properties,
}

/// The launches a request comes to: each folder of `uris`
/// ([`Shown::Folders`]), or the folders holding them, in the order first
/// named, each with its items to select. Only local files: a `file:` URI
/// of this machine, or an absolute path, as some programs send; anything
/// else is passed over (an address on a server is the remote plugin's
/// to open, ADR-077). The desktop's token brings the window of the last
/// forward. A folder named twice is one launch, an item twice once.
#[must_use]
pub fn openings(shown: Shown, uris: &[String], token: Option<String>) -> Vec<Opening> {
    let paths = uris.iter().filter_map(|uri| local(uri));
    let mut openings: Vec<Opening> = Vec::new();
    let mut at: HashMap<PathBuf, usize> = HashMap::new();
    let mut seen: HashSet<PathBuf> = HashSet::new();
    for path in paths {
        if !seen.insert(path.clone()) {
            continue;
        }
        let (folder, name) = match (shown, path.parent(), path.file_name()) {
            (Shown::Folders, _, _) | (_, None, _) | (_, _, None) => (path, None),
            (_, Some(parent), Some(name)) => (parent.to_path_buf(), Some(name.to_owned())),
        };
        let index = *at.entry(folder.clone()).or_insert_with(|| {
            openings.push(Opening {
                location: Some(folder.into_os_string()),
                properties: shown == Shown::Properties,
                ..Opening::default()
            });
            openings.len() - 1
        });
        if let Some(name) = name {
            openings[index].select.push(name);
        }
    }
    if let Some(last) = openings.last_mut() {
        last.token = token;
    }
    openings
}

/// The local path `uri` names: a `file:` URI of this machine, else an
/// absolute path as given.
fn local(uri: &str) -> Option<PathBuf> {
    if uri.starts_with("file:") {
        return crate::uri::local_path_of(uri);
    }
    let path = PathBuf::from(uri);
    path.is_absolute().then_some(path)
}

/// Who shows the files other programs show in their folder, as the bus
/// says now ([`shower`]).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Shower {
    /// This program.
    Here,
    /// Another program, by the path it was started as
    /// (`/usr/bin/dolphin`), or none where it cannot be read: skwir waits
    /// behind it while chosen.
    Program(Option<PathBuf>),
    /// No one: the bus starts what its files name, or the asker opens
    /// the folder itself.
    Nobody,
    /// Not known: another system, or no session bus.
    Unknown,
}

/// Who holds `org.freedesktop.FileManager1` now: asked of the bus, which
/// answers at once (it is the bus's own question, not another program's).
/// On a worker.
#[must_use]
pub fn shower() -> Shower {
    #[cfg(target_os = "linux")]
    {
        linux::shower()
    }
    #[cfg(not(target_os = "linux"))]
    {
        Shower::Unknown
    }
}

/// What follows the user's choice ([`follow`]): dropped, it stops.
pub struct Following {
    #[cfg(target_os = "linux")]
    _watcher: Option<skwir_core::fs::Settled>,
}

/// Follow the user's choice for the program `answering` belongs to: asked
/// now, then each time the user's lists of defaults change (Open With, the
/// desktop's settings, `xdg-mime`), on the watcher's own thread: the
/// program answers other programs while skwir is the default for
/// folders, and the bus's file is kept as the choice says. Whether skwir
/// is chosen now, and what follows. Reads the lists, a file, and asks the
/// bus: call it on a worker. Elsewhere than on Linux: never chosen,
/// nothing followed.
pub fn follow(answering: &crate::handover::Answering) -> (bool, Following) {
    #[cfg(target_os = "linux")]
    {
        linux::follow(answering)
    }
    #[cfg(not(target_os = "linux"))]
    {
        let _ = answering;
        (false, Following {})
    }
}

#[cfg(target_os = "linux")]
mod linux {
    use std::ffi::OsString;
    use std::path::{Path, PathBuf};

    use super::{FOLDERS, Following, STARTED_BY_BUS};
    use crate::apps::Application;
    use crate::handover::Answering;

    /// The service the file starts skwir for.
    const NAME: &str = "org.freedesktop.FileManager1";
    /// The file's name in the user's services folder.
    const FILE: &str = "org.freedesktop.FileManager1.service";
    /// The first line of the file skwir writes: how it knows its own,
    /// which alone it changes or removes.
    const MARK: &str =
        "# Written by skwir while it is the default for folders (ADR-140); removed when it is not.";
    /// How long a burst of changes to the lists is let settle.
    const SETTLE: std::time::Duration = std::time::Duration::from_millis(200);

    pub(super) fn shower() -> super::Shower {
        use super::Shower;
        let Ok(bus) = crate::bus::session() else {
            return Shower::Unknown;
        };
        let Ok(dbus) = zbus::blocking::fdo::DBusProxy::new(&bus) else {
            return Shower::Unknown;
        };
        let Ok(name) = zbus::names::BusName::try_from(NAME) else {
            return Shower::Unknown;
        };
        match dbus.get_connection_unix_process_id(name) {
            Ok(pid) if pid == std::process::id() => Shower::Here,
            Ok(pid) => Shower::Program(started_as(pid)),
            Err(zbus::fdo::Error::NameHasNoOwner(_)) => Shower::Nobody,
            Err(error) => {
                tracing::debug!("who shows files is not known: {error}");
                Shower::Unknown
            }
        }
    }

    /// The path the process `pid` was started as: its first argument.
    fn started_as(pid: u32) -> Option<PathBuf> {
        let line = std::fs::read(format!("/proc/{pid}/cmdline")).ok()?;
        let first = line.split(|b| *b == 0).next().filter(|f| !f.is_empty())?;
        Some(PathBuf::from(skwir_core::os_path::from_bytes(
            first.to_vec(),
        )))
    }

    /// Skwir's desktop entry, when it is the default the desktop names
    /// for folders. Reads the lists: on a worker.
    pub(super) fn chosen() -> Option<Application> {
        let own = format!("{}.desktop", skwir_core::APP_ID);
        crate::apps::named_default(FOLDERS).filter(|app| app.id == own)
    }

    pub(super) fn follow(answering: &Answering) -> (bool, Following) {
        let chosen = check(answering);
        let answering = answering.clone();
        let lists = crate::apps::user_lists();
        let mut folders: Vec<PathBuf> = lists
            .iter()
            .filter_map(|list| list.parent().map(Path::to_path_buf))
            .collect();
        folders.dedup();
        let names: Vec<OsString> = lists
            .iter()
            .filter_map(|list| list.file_name().map(ToOwned::to_owned))
            .collect();
        let relevant = move |path: &Path| {
            path.file_name()
                .is_some_and(|name| names.iter().any(|n| n == name))
        };
        let watcher =
            skwir_core::fs::Settled::start("skwir-defaults", SETTLE, relevant, move || {
                check(&answering);
                true
            })
            .map(|mut watcher| {
                for folder in &folders {
                    watcher.watch(folder);
                }
                watcher
            });
        (chosen, Following { _watcher: watcher })
    }

    /// Ask whether skwir is chosen, and answer and keep the bus's file as
    /// it says: whether it is.
    fn check(answering: &Answering) -> bool {
        let chosen = chosen();
        answering.set(chosen.is_some());
        let Some(path) = path() else {
            return chosen.is_some();
        };
        match keep_at(&path, chosen.as_ref()) {
            Kept::Unchanged => {}
            Kept::Written => {
                tracing::info!(file = %path.display(), "the bus starts skwir to show files");
                reload_bus();
            }
            Kept::Removed => {
                tracing::info!(file = %path.display(), "the bus no longer starts skwir to show files");
                reload_bus();
            }
            Kept::Gone(why) => {
                tracing::info!(file = %path.display(), "the bus no longer starts skwir to show files: {why}");
                reload_bus();
            }
            Kept::NotOurs => {
                tracing::debug!(file = %path.display(), "a file of the user's own starts what shows files");
            }
            Kept::Cannot(why) => {
                tracing::info!(file = %path.display(), "the bus cannot be asked to start skwir: {why}");
            }
        }
        chosen.is_some()
    }

    /// Where the bus reads the user's services: read before the system's
    /// folders (`<standard_session_servicedirs/>`).
    fn path() -> Option<PathBuf> {
        crate::xdg::data_home().map(|data| data.join("dbus-1").join("services").join(FILE))
    }

    /// What [`keep_at`] did.
    #[derive(Debug, PartialEq, Eq)]
    pub(super) enum Kept {
        Unchanged,
        Written,
        Removed,
        /// Skwir's, removed although chosen, as it would start nothing:
        /// why it cannot be made.
        Gone(String),
        /// A file of that name that skwir did not write: left as it is.
        NotOurs,
        /// The file could not be written or read, or the program not
        /// named: why.
        Cannot(String),
    }

    /// Keep the file at `path` as `chosen` says: skwir's, starting the
    /// program its desktop entry runs, while it is chosen; else none of
    /// skwir's. A file of the user's own is never touched; one of skwir's
    /// that can no longer be made (its program gone from `PATH`) goes, as
    /// it would start nothing.
    pub(super) fn keep_at(path: &Path, chosen: Option<&Application>) -> Kept {
        let now = match crate::file::read_regular_text(path, crate::file::DESKTOP_FILE) {
            Ok(text) => Some(text),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
            Err(error) => return Kept::Cannot(error.to_string()),
        };
        if now.as_deref().is_some_and(|text| !text.starts_with(MARK)) {
            return Kept::NotOurs;
        }
        let wanted = chosen.map(text);
        match (now, wanted) {
            (None, None) => Kept::Unchanged,
            (Some(_), None) => remove(path),
            (None, Some(Err(why))) => Kept::Cannot(why),
            (Some(_), Some(Err(why))) => match remove(path) {
                Kept::Removed => Kept::Gone(why),
                other => other,
            },
            (Some(now), Some(Ok(text))) if now == text => Kept::Unchanged,
            (_, Some(Ok(text))) => match write(path, &text) {
                Ok(()) => Kept::Written,
                Err(error) => Kept::Cannot(error.to_string()),
            },
        }
    }

    fn remove(path: &Path) -> Kept {
        match std::fs::remove_file(path) {
            Ok(()) => Kept::Removed,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Kept::Unchanged,
            Err(error) => Kept::Cannot(error.to_string()),
        }
    }

    fn write(path: &Path, text: &str) -> std::io::Result<()> {
        let folder = path
            .parent()
            .ok_or_else(|| std::io::Error::other("a file in no folder"))?;
        std::fs::create_dir_all(folder)?;
        let mut staged = tempfile::NamedTempFile::new_in(folder)?;
        std::io::Write::write_all(&mut staged, text.as_bytes())?;
        skwir_core::fs::file::take_place(staged, path)
    }

    /// The file that has the bus start `app`'s program with
    /// [`STARTED_BY_BUS`]: its command line as its entry gives it without
    /// files, its program as `PATH` finds it (a profile's link, never a
    /// build's own path, which an update or a test build leaves behind).
    /// Refused, with why, where the program is not found or a word would
    /// need quoting in the file's grammar.
    pub(super) fn text(app: &Application) -> Result<String, String> {
        let line = app
            .command_lines(&[])
            .into_iter()
            .next()
            .unwrap_or_default();
        let Some((program, rest)) = line.split_first() else {
            return Err(format!("{} names no program", app.id));
        };
        let found = skwir_core::os_path::find_program(program)
            .ok_or_else(|| format!("{} is not on PATH", program.to_string_lossy()))?;
        let mut words: Vec<OsString> = vec![found.into_os_string()];
        words.extend(rest.iter().cloned());
        words.push(STARTED_BY_BUS.into());
        let exec = words
            .iter()
            .map(|word| {
                word.to_str()
                    .filter(|word| plain(word))
                    .ok_or_else(|| format!("{} would need quoting", word.to_string_lossy()))
            })
            .collect::<Result<Vec<&str>, String>>()?
            .join(" ");
        Ok(format!(
            "{MARK}\n[D-BUS Service]\nName={NAME}\nExec={exec}\n"
        ))
    }

    /// A word the bus reads as it is, in dbus-daemon's grammar and
    /// dbus-broker's alike: nothing a shell or a desktop file quotes.
    fn plain(word: &str) -> bool {
        !word.is_empty()
            && word
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b"/._+-=:@,".contains(&b))
    }

    /// Ask the session bus to read its files again: dbus-broker does not
    /// watch them, dbus-daemon does and answers this too.
    fn reload_bus() {
        let reloaded = crate::bus::session().and_then(|bus| {
            zbus::blocking::fdo::DBusProxy::new(&bus)?.reload_config()?;
            Ok(())
        });
        if let Err(error) = reloaded {
            tracing::debug!("the bus did not read its files again: {error}");
        }
    }
}

#[cfg(test)]
mod tests {
    #[cfg(unix)]
    use super::*;
    #[cfg(unix)]
    use std::ffi::OsString;

    // The paths the tests name are Unix's.
    #[cfg(unix)]
    fn opening(location: &str, select: &[&str], properties: bool) -> Opening {
        Opening {
            location: Some(location.into()),
            select: select.iter().map(OsString::from).collect(),
            properties,
            ..Opening::default()
        }
    }

    /// Items are grouped by their folder, in the order first named, each
    /// named once; the token goes with the last; what is no local file is
    /// passed over.
    #[cfg(unix)]
    #[test]
    fn a_request_comes_to_a_launch_per_folder() {
        let uris = [
            "file:///srv/Field%20Guide/it's%20a%20draft.txt",
            "file:///srv/Maps/tr%C3%B6isi%C3%A8me.png",
            "/srv/Field Guide/second.md",
            "file:///srv/Field%20Guide/it's%20a%20draft.txt",
            "sftp://host/srv/elsewhere.txt",
            "relative/name.txt",
            "file:///",
        ]
        .map(String::from);
        let mut last = opening("/", &[], false);
        last.token = Some("token".into());
        assert_eq!(
            openings(Shown::Items, &uris, Some("token".into())),
            [
                opening(
                    "/srv/Field Guide",
                    &["it's a draft.txt", "second.md"],
                    false
                ),
                opening("/srv/Maps", &["tröisième.png"], false),
                last,
            ]
        );
        let props = openings(Shown::Properties, &uris[..2], None);
        assert!(props.iter().all(|o| o.properties && o.token.is_none()));
        let folders = ["file:///srv/Maps/", "/srv/Field Guide"].map(String::from);
        assert_eq!(
            openings(Shown::Folders, &folders, None),
            [
                opening("/srv/Maps/", &[], false),
                opening("/srv/Field Guide", &[], false)
            ]
        );
        assert!(openings(Shown::Items, &["sftp://host/x".into()], None).is_empty());
    }

    #[cfg(target_os = "linux")]
    mod linux {
        use super::super::linux::{Kept, keep_at, text};
        use crate::apps::Application;
        use std::path::Path;

        fn entry(exec: &str) -> Application {
            Application {
                id: "org.skwir.skwir.desktop".into(),
                name: "skwir".into(),
                generic_name: None,
                icon: None,
                exec: exec.into(),
                terminal: false,
                working_dir: None,
                mime_types: vec!["inode/directory".into()],
                shown: true,
                source: "/apps/org.skwir.skwir.desktop".into(),
            }
        }

        /// A program of the test's own: a script, found by its path.
        fn program(folder: &Path, name: &str) -> std::path::PathBuf {
            use std::os::unix::fs::PermissionsExt as _;
            let path = folder.join(name);
            std::fs::write(&path, "#!/bin/sh\n").unwrap();
            std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
            path
        }

        /// The file starts the program the entry runs, its arguments kept,
        /// with `--file-manager`; a program not found, or a word that would
        /// need quoting, is refused.
        #[test]
        fn the_file_starts_what_the_entry_runs() {
            let folder = tempfile::tempdir().unwrap();
            let skwir = program(folder.path(), "skwir");
            let made = text(&entry(&format!("{} --verbose %f", skwir.display()))).unwrap();
            assert_eq!(
                made.lines().skip(1).collect::<Vec<_>>(),
                [
                    "[D-BUS Service]",
                    "Name=org.freedesktop.FileManager1",
                    &format!("Exec={} --verbose --file-manager", skwir.display()),
                ]
            );
            assert!(text(&entry("/nowhere/skwir %f")).is_err());
            let spaced = std::fs::create_dir(folder.path().join("two words"));
            spaced.unwrap();
            let spaced = program(&folder.path().join("two words"), "skwir");
            assert!(
                text(&entry(&format!("\"{}\" %f", spaced.display()))).is_err(),
                "a path the file would need quoted"
            );
        }

        /// Written while chosen, left as it is when it says the same,
        /// removed when not chosen; a file of the user's own of that name
        /// never touched.
        #[test]
        fn the_file_is_kept_as_the_choice_says() {
            let folder = tempfile::tempdir().unwrap();
            let skwir = program(folder.path(), "skwir");
            let app = entry(&format!("{} %f", skwir.display()));
            let path = folder.path().join("services").join("x.service");
            assert_eq!(keep_at(&path, None), Kept::Unchanged);
            assert_eq!(keep_at(&path, Some(&app)), Kept::Written);
            assert_eq!(std::fs::read_to_string(&path).unwrap(), text(&app).unwrap());
            assert_eq!(keep_at(&path, Some(&app)), Kept::Unchanged);
            assert_eq!(keep_at(&path, None), Kept::Removed);
            assert!(!path.exists());
            let own = "[D-BUS Service]\nName=org.freedesktop.FileManager1\nExec=/opt/other\n";
            std::fs::write(&path, own).unwrap();
            assert_eq!(keep_at(&path, Some(&app)), Kept::NotOurs);
            assert_eq!(keep_at(&path, None), Kept::NotOurs);
            assert_eq!(std::fs::read_to_string(&path).unwrap(), own);
        }

        /// One of skwir's whose program is gone goes: it would start
        /// nothing.
        #[test]
        fn a_file_that_would_start_nothing_goes() {
            let folder = tempfile::tempdir().unwrap();
            let skwir = program(folder.path(), "skwir");
            let path = folder.path().join("x.service");
            assert_eq!(
                keep_at(&path, Some(&entry(&format!("{} %f", skwir.display())))),
                Kept::Written
            );
            std::fs::remove_file(&skwir).unwrap();
            let gone = entry(&format!("{} %f", skwir.display()));
            assert!(matches!(keep_at(&path, Some(&gone)), Kept::Gone(_)));
            assert!(!path.exists());
            assert!(matches!(keep_at(&path, Some(&gone)), Kept::Cannot(_)));
        }
    }
}
