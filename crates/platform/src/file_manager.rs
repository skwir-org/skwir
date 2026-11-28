//! Showing folders and files for other programs (ADR-140). While skwir
//! is the default the desktop names for folders, the program that takes
//! launches over answers `org.freedesktop.FileManager1` ("Show in
//! folder": [`crate::handover::Answering`]) and keeps the file that has
//! the session bus start skwir for it when none runs; when it is not, it
//! answers nothing and takes its file back. The choice is the desktop's
//! own: skwir keeps none of its own.
//!
//! Linux's bus integration alone: elsewhere [`follow`] follows nothing.
//! macOS open URLs use [`Shown::Opened`] through the UI's launch queue
//! (ADR-140, 7).

use std::collections::{HashMap, HashSet};
use std::ffi::{OsStr, OsString};
use std::path::{Path, PathBuf};

use crate::handover::Opening;

/// The type folders are, whose default is the choice.
pub const FOLDERS: &str = "inode/directory";

/// The argument the bus starts skwir with: the program starts without a
/// window and opens one for the request (ADR-140, 5).
pub const STARTED_BY_BUS: &str = "--file-manager";

/// Why a program started to answer other programs (with
/// [`STARTED_BY_BUS`], or by the portal for their dialogs, ADR-141) ends
/// at once off Linux, where neither is written (ADR-140, 7): what is not
/// written for a system yet, said as `system::not_yet_words` says it.
pub fn not_yet() -> skwir_core::words::Words {
    skwir_core::words!(
        file_manager_not_yet = "Answering other programs is not available on { $system } yet",
        system = skwir_core::system::named(),
        note = "Why skwir, started to show files for other programs or to answer their dialogs, ends at once on a system where neither is written yet. system: its name (macOS, Windows)."
    )
}

/// What a request asks shown.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Shown {
    /// `ShowFolders`: each folder opened.
    Folders,
    /// `ShowItems`: the folder of each item opened, its items selected.
    Items,
    /// `ShowItemProperties`: as [`Shown::Items`], Properties shown.
    Properties,
    /// Open URLs: ordinary folders opened, files and macOS packages
    /// selected in their parent. Reads the filesystem: call on a worker.
    Opened,
}

/// What a named launch opens: a file or macOS package is selected in its
/// parent, an ordinary folder opens as itself. Unknown paths and plugin
/// locations stay as given. Relative paths are read against `cwd`.
///
/// Reads the filesystem (and macOS's package registry): never call on the
/// window thread. Shared by the CLI and the application's open-URL handler.
#[must_use]
pub fn named(arg: &OsStr, cwd: &Path) -> (OsString, Option<OsString>) {
    use skwir_core::entry::EntryKind;

    let path = cwd.join(arg);
    let reveal = skwir_core::fs::stat::stat_path(&path).is_some_and(|entry| match entry.opens() {
        EntryKind::Directory => package(&path),
        // As with the CLI's previous metadata call, a link whose target
        // cannot be read is left as given.
        EntryKind::Symlink | EntryKind::Unknown => false,
        EntryKind::File
        | EntryKind::Fifo
        | EntryKind::Socket
        | EntryKind::CharDevice
        | EntryKind::BlockDevice => true,
    });
    match (reveal, path.parent(), path.file_name()) {
        (true, Some(folder), Some(name)) => {
            // AppKit may deliver a different Unicode spelling than the
            // directory keeps. Selection names must match its entries,
            // not the URL's spelling. Ask for this entry, never resolve
            // its links or scan the parent to find an equivalent name.
            #[cfg(target_os = "macos")]
            let name = skwir_core::fs::stat::stored_name(&path).unwrap_or_else(|| name.to_owned());
            #[cfg(not(target_os = "macos"))]
            let name = name.to_owned();
            (folder.as_os_str().to_owned(), Some(name))
        }
        _ => (arg.to_owned(), None),
    }
}

/// Ask the system, not a list of extensions: document packages and app
/// bundles are directories on disk but items to reveal in their parent.
fn package(path: &Path) -> bool {
    #[cfg(target_os = "macos")]
    {
        path.to_str().is_some_and(|path| {
            objc2::rc::autoreleasepool(|_| {
                objc2_app_kit::NSWorkspace::sharedWorkspace()
                    .isFilePackageAtPath(&objc2_foundation::NSString::from_str(path))
            })
        })
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = path;
        false
    }
}

/// The launches a request comes to: each folder of `uris`
/// ([`Shown::Folders`]), or the folders holding them, in the order first
/// named, each with its items to select. Only local files: a `file:` URI
/// of this machine, or an absolute path, as some programs send; anything
/// else is passed over (an address on a server is the remote plugin's
/// to open, ADR-077). The desktop's token brings the window of the last
/// forward. A folder named twice is one launch, an item twice once.
/// [`Shown::Opened`] reads each path's type: call it on a worker.
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
            (Shown::Opened, _, _) => {
                let (folder, name) = named(path.as_os_str(), Path::new(""));
                (PathBuf::from(folder), name)
            }
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
        // These requests name filesystem paths, not URL queries or
        // fragments. Literal #, ? and % in names must be percent-encoded.
        if uri.contains(['?', '#']) {
            return None;
        }
        let mut bytes = uri.as_bytes().iter();
        while let Some(byte) = bytes.next() {
            if *byte == b'%'
                && !(bytes.next().is_some_and(u8::is_ascii_hexdigit)
                    && bytes.next().is_some_and(u8::is_ascii_hexdigit))
            {
                return None;
            }
        }
        return crate::uri::local_path_of(uri)
            .filter(|path| !path.as_os_str().as_encoded_bytes().contains(&0));
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
    #[cfg(target_os = "linux")]
    again: linux::Again,
}

impl Following {
    /// What asks the choice again ([`Again`]), held by no lock of the
    /// caller's while it reads.
    #[must_use]
    pub fn again(&self) -> Again {
        Again {
            #[cfg(target_os = "linux")]
            inner: self.again.clone(),
        }
    }
}

impl Drop for Following {
    fn drop(&mut self) {
        #[cfg(target_os = "linux")]
        self.again.stop();
    }
}

/// Asks the choice again, as [`follow`] does at first ([`Again::check`]),
/// until what it came from is dropped.
#[derive(Clone)]
pub struct Again {
    #[cfg(target_os = "linux")]
    inner: linux::Again,
}

impl Again {
    /// For what no watch tells: the system's lists (`$XDG_CONFIG_DIRS`, the
    /// data folders'), which a package or a profile switched (NixOS's, a
    /// link above the folder) changes. Asked as a window comes to the
    /// front. Skipped while a reading of the choice runs, which reads them
    /// too, and once following stopped. Reads the lists and a file, and
    /// asks the bus only where what it answers changed: on a worker.
    /// Nothing elsewhere than on Linux.
    pub fn check(&self) {
        #[cfg(target_os = "linux")]
        self.inner.check();
    }
}

/// Follow who holds `org.freedesktop.FileManager1` (what shows other
/// programs' files in their folder), until `stop`: `changed` is called on
/// a thread of its own once listening (so nothing between the caller's
/// last [`shower`] and then is missed), then each time its owner changes,
/// and must return at once (ask [`shower`] on a worker). For what is told
/// only while another program holds it (the Settings row saying so).
/// Nothing elsewhere than on Linux, or without a session bus.
pub fn follow_shower(stop: &skwir_core::CancelToken, changed: impl Fn() + Send + 'static) {
    #[cfg(target_os = "linux")]
    linux::follow_shower(stop, changed);
    #[cfg(not(target_os = "linux"))]
    {
        let _ = (stop, changed);
    }
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
pub(crate) mod linux {
    use std::ffi::OsString;
    use std::path::{Path, PathBuf};
    use std::sync::Arc;

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

    /// One reading of the choice at a time, whoever asks (the watcher, a
    /// window come to the front): one asked while another runs is that
    /// one.
    #[derive(Clone)]
    pub(super) struct Again {
        answering: Answering,
        reading: Arc<parking_lot::Mutex<()>>,
        /// Following goes on: once it stops, nothing is read nor kept.
        live: Arc<std::sync::atomic::AtomicBool>,
    }

    impl Again {
        /// Read it, waiting for a reading under way (the watcher's: what
        /// the lists say after it).
        fn read(&self) -> bool {
            let _reading = self.reading.lock();
            self.is_live() && check(&self.answering)
        }

        /// Read it, unless one is under way.
        pub(super) fn check(&self) {
            if let Some(_reading) = self.reading.try_lock()
                && self.is_live()
            {
                check(&self.answering);
            }
        }

        pub(super) fn stop(&self) {
            self.live.store(false, std::sync::atomic::Ordering::Release);
        }

        fn is_live(&self) -> bool {
            self.live.load(std::sync::atomic::Ordering::Acquire)
        }
    }

    pub(super) fn follow(answering: &Answering) -> (bool, Following) {
        let again = Again {
            answering: answering.clone(),
            reading: Arc::default(),
            live: Arc::new(std::sync::atomic::AtomicBool::new(true)),
        };
        let chosen = again.read();
        let on_change = again.clone();
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
                on_change.read();
                true
            })
            .map(|mut watcher| {
                for folder in &folders {
                    watcher.watch(folder);
                }
                watcher
            });
        (
            chosen,
            Following {
                _watcher: watcher,
                again,
            },
        )
    }

    /// What says the name's owner changed.
    pub(crate) fn owner_changes() -> zbus::Result<zbus::MatchRule<'static>> {
        Ok(zbus::MatchRule::builder()
            .msg_type(zbus::message::Type::Signal)
            .sender("org.freedesktop.DBus")?
            .interface("org.freedesktop.DBus")?
            .member("NameOwnerChanged")?
            .arg(0, NAME)?
            .build())
    }

    pub(super) fn follow_shower(
        stop: &skwir_core::CancelToken,
        changed: impl Fn() + Send + 'static,
    ) {
        let rule = match owner_changes() {
            Ok(rule) => rule,
            Err(error) => {
                tracing::debug!("who shows files is asked, not followed: {error}");
                return;
            }
        };
        match crate::bus::session() {
            Ok(bus) => follow_shower_on(bus, rule, stop, changed),
            Err(error) => tracing::debug!("who shows files is asked, not followed: {error}"),
        }
    }

    /// [`follow_shower`] on `bus`, with `rule`, the owner's changes of the
    /// name.
    pub(crate) fn follow_shower_on(
        bus: zbus::blocking::Connection,
        rule: zbus::MatchRule<'static>,
        stop: &skwir_core::CancelToken,
        changed: impl Fn() + Send + 'static,
    ) {
        let stop = stop.clone();
        let followed = std::thread::Builder::new()
            .name("skwir-shower".into())
            .spawn(move || {
                // Both on this thread: told once listening, then at each change.
                let changed = &changed;
                crate::bus::listen(&bus, vec![rule], &stop, changed, |_| changed());
            });
        if let Err(error) = followed {
            tracing::debug!("who shows files is asked, not followed: {error}");
        }
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
    use super::*;

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

    /// The macOS URL handler must not turn a file into a folder location.
    /// One request selects all the files of each parent, not one window
    /// per file; ordinary folders still open as folders.
    #[test]
    fn opened_urls_reveal_files_together_and_open_folders() {
        let root = tempfile::tempdir().unwrap();
        let folder = root.path().join("A traveller's notes");
        let other = root.path().join("Maps");
        std::fs::create_dir(&folder).unwrap();
        std::fs::create_dir(&other).unwrap();
        let first = folder.join("draft #1.txt");
        let second = folder.join("tröisième %.txt");
        let third = other.join("map.txt");
        for file in [&first, &second, &third] {
            std::fs::write(file, b"example").unwrap();
        }
        let uris =
            [&first, &second, &first, &third, &folder].map(|path| crate::uri::file_uri(path));
        let expected = |folder: &Path, names: &[&str]| Opening {
            location: Some(folder.as_os_str().to_owned()),
            select: names.iter().map(OsString::from).collect(),
            ..Opening::default()
        };
        assert_eq!(
            openings(Shown::Opened, &uris, None),
            [
                expected(&folder, &["draft #1.txt", "tröisième %.txt"]),
                expected(&other, &["map.txt"]),
            ]
        );
        assert_eq!(
            openings(Shown::Opened, &[crate::uri::file_uri(&other)], None),
            [expected(&other, &[])]
        );
        // The CLI and open-URL requests have the same owner; relative
        // arguments resolve against the launch's cwd, not the process's.
        assert_eq!(
            named(
                Path::new("A traveller's notes/draft #1.txt").as_os_str(),
                root.path()
            ),
            (folder.into_os_string(), Some("draft #1.txt".into()))
        );
        assert_eq!(
            named(OsStr::new("Maps"), root.path()),
            ("Maps".into(), None)
        );
        assert_eq!(
            named(OsStr::new("missing.txt"), root.path()),
            ("missing.txt".into(), None)
        );
    }

    #[test]
    fn opened_urls_ignore_foreign_and_malformed_addresses() {
        let urls = [
            "sftp://server.example/notes.txt",
            "https://example.com/notes.txt",
            "file://server.example/notes.txt",
            "file:",
            "file://",
            "file:///notes.txt?query",
            "file:///notes.txt#fragment",
            "file:///bad%escape.txt",
            "file:///bad%.txt",
            "file:///bad%00name.txt",
            "relative/notes.txt",
        ]
        .map(String::from);
        assert!(openings(Shown::Opened, &urls, None).is_empty());
    }

    #[cfg(unix)]
    #[test]
    fn opened_links_keep_their_names_and_open_directory_targets() {
        let root = tempfile::tempdir().unwrap();
        let file = root.path().join("original.txt");
        let folder = root.path().join("folder");
        let file_link = root.path().join("file link.txt");
        let folder_link = root.path().join("folder link");
        let broken = root.path().join("broken link");
        std::fs::write(&file, b"example").unwrap();
        std::fs::create_dir(&folder).unwrap();
        std::os::unix::fs::symlink(&file, &file_link).unwrap();
        std::os::unix::fs::symlink(&folder, &folder_link).unwrap();
        std::os::unix::fs::symlink(root.path().join("gone"), &broken).unwrap();
        assert_eq!(
            named(file_link.as_os_str(), Path::new("")),
            (
                root.path().as_os_str().to_owned(),
                Some("file link.txt".into())
            )
        );
        assert_eq!(
            named(folder_link.as_os_str(), Path::new("")),
            (folder_link.into_os_string(), None)
        );
        assert_eq!(
            named(broken.as_os_str(), Path::new("")),
            (broken.into_os_string(), None)
        );
    }

    /// AppKit decomposes names in delivered URLs, while APFS keeps the
    /// spelling used to create an entry. The selection must use the
    /// listing's bytes in either direction, including each link's own
    /// name rather than another name of its file or its target.
    #[cfg(target_os = "macos")]
    #[test]
    fn opened_macos_names_select_the_entries_spelling_on_disk() {
        let root = tempfile::tempdir().unwrap();
        for (group, names) in [
            (
                "NFC",
                [
                    ("Café notes.txt", "Cafe\u{301} notes.txt"),
                    ("Liñk.txt", "Lin\u{303}k.txt"),
                    ("Hárd.txt", "Ha\u{301}rd.txt"),
                ],
            ),
            (
                "NFD",
                [
                    ("Cafe\u{301} notes.txt", "Café notes.txt"),
                    ("Lin\u{303}k.txt", "Liñk.txt"),
                    ("Ha\u{301}rd.txt", "Hárd.txt"),
                ],
            ),
        ] {
            let folder = root.path().join(group);
            std::fs::create_dir(&folder).unwrap();
            let file = folder.join(names[0].0);
            std::fs::write(&file, b"example").unwrap();
            std::os::unix::fs::symlink(&file, folder.join(names[1].0)).unwrap();
            std::fs::hard_link(&file, folder.join(names[2].0)).unwrap();
            // A filesystem may itself choose a spelling (HFS+ decomposes
            // names). Read the actual listing, independently of named().
            let listed: HashSet<OsString> = std::fs::read_dir(&folder)
                .unwrap()
                .map(|entry| entry.unwrap().file_name())
                .collect();
            let mut urls = Vec::new();
            let mut selected = Vec::new();
            for (made, sent) in names {
                let actual = if listed.contains(OsStr::new(made)) {
                    made
                } else {
                    assert!(listed.contains(OsStr::new(sent)));
                    sent
                };
                let requested = folder.join(sent);
                assert_eq!(
                    named(requested.as_os_str(), Path::new("")),
                    (folder.as_os_str().to_owned(), Some(actual.into())),
                    "the CLI selects the entry, not the sender's spelling: {group}/{sent}"
                );
                urls.push(crate::uri::file_uri(&requested));
                selected.push(OsString::from(actual));
            }
            assert_eq!(
                openings(Shown::Opened, &urls, None),
                [Opening {
                    location: Some(folder.into_os_string()),
                    select: selected,
                    ..Opening::default()
                }],
                "one open-URL batch selects all three entries, including both links"
            );
        }
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn opened_macos_packages_reveal_instead_of_browsing_the_bundle() {
        let root = tempfile::tempdir().unwrap();
        let package = root.path().join("Example.app");
        std::fs::create_dir_all(package.join("Contents")).unwrap();
        std::fs::write(
            package.join("Contents/Info.plist"),
            b"<?xml version=\"1.0\"?><plist version=\"1.0\"><dict>\
              <key>CFBundlePackageType</key><string>APPL</string>\
              <key>CFBundleIdentifier</key><string>org.example.fixture</string>\
              </dict></plist>",
        )
        .unwrap();
        let opened = openings(Shown::Opened, &[crate::uri::file_uri(&package)], None);
        assert_eq!(
            opened,
            [Opening {
                location: Some(root.path().as_os_str().to_owned()),
                select: vec!["Example.app".into()],
                ..Opening::default()
            }]
        );
        assert_eq!(
            named(package.as_os_str(), Path::new("")),
            (
                root.path().as_os_str().to_owned(),
                Some("Example.app".into())
            )
        );
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
