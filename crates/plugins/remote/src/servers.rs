//! The servers added by their address (ADR-086, amended 2026-10-24): an
//! SFTP, SMB, WebDAV, FTP, NFS or AFP server Add Server made, kept in
//! Skwir's own list (`remote.kept_servers`, the plugin's data) or in KDE's
//! network folders (`$XDG_DATA_HOME/remoteview`, Dolphin's `remote:/`), as
//! `remote.servers_kept_in` says: KDE's on KDE's desktop where KIO opens
//! the kind, Skwir's elsewhere. Both are read, whichever is chosen: each
//! server is changed and removed where it is kept. They stand in the
//! sidebar's Network section with rclone's remotes, never among the
//! bookmarks, which are what the user bookmarked.

use std::path::{Path, PathBuf};

use skwir_core::app::App;
use skwir_core::settings::SettingSpec;
use skwir_core::words;
use skwir_core::words::Words;

use crate::address::{Address, Scheme};
use crate::connector::{Server, Source};

/// Skwir's own list of servers: each one's name and address.
pub const LIST: &str = "remote.kept_servers";

/// Where a server added by its address is kept.
pub const KEPT_IN: &str = "remote.servers_kept_in";

/// Who keeps a server the Network section lists.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Kept {
    /// The tool that reads it, which changes it.
    Tool(Keeper),
    /// Skwir's own list.
    Skwir,
    /// KDE's network folders.
    Kde,
}

/// The tool that keeps a server it reads: where, said by [`Kept::words`].
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Keeper {
    /// rclone's configuration.
    Rclone,
    /// Dolphin's places, read through KIO.
    Dolphin,
    /// The system's fstab.
    Fstab,
}

impl Kept {
    /// Where it is kept, said: "in Skwir's list".
    pub fn words(self) -> Words {
        match self {
            Kept::Tool(Keeper::Rclone) => words!(
                remote_servers_kept_rclone = "in rclone's configuration",
                note = "Where a server of the Network section is kept, said after its kind or address."
            ),
            Kept::Tool(Keeper::Dolphin) => words!(
                remote_servers_kept_dolphin = "one of Dolphin's places",
                note = "Where a server of the Network section is kept, said after its kind or \
                        address: Dolphin is KDE's file manager."
            ),
            Kept::Tool(Keeper::Fstab) => words!(
                remote_servers_kept_fstab = "in the system's fstab",
                note = "Where a server of the Network section is kept, said after its kind or \
                        address: fstab is the system's table of mounts."
            ),
            Kept::Skwir => words!(
                remote_servers_kept_skwir = "in Skwir's list",
                note = "Where a server of the Network section is kept, said after its kind or address."
            ),
            Kept::Kde => words!(
                remote_servers_kept_kde = "in KDE's network folders",
                note = "Where a server of the Network section is kept, said after its kind or address."
            ),
        }
    }

    /// Whether Skwir keeps it, and changes it: one added by its address.
    pub fn by_address(self) -> bool {
        matches!(self, Kept::Skwir | Kept::Kde)
    }
}

/// A server of Skwir's own list, as the plugin's data holds it.
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
struct Entry {
    name: String,
    address: String,
}

pub fn declare(app: &App) {
    app.settings.declare(
        SettingSpec::choice_words(
            KEPT_IN,
            words!(remote_servers_kept_in = "Where servers are kept"),
            (
                words!(
                    remote_servers_kept_in_description = "Where a server added by its address is kept."
                ),
                words!(
                    remote_servers_kept_in_description_more = "Automatic: in KDE's network folders on KDE's desktop, where Dolphin lists them too, and in Skwir's own list elsewhere; a kind KIO does not open (FTPS, AFP) always in Skwir's.",
                    note = "What a setting's description says after its first sentence, in its tooltip."
                ),
            ),
            "automatic",
            vec![
                (
                    "automatic",
                    words!(
                        remote_servers_kept_automatic = "Automatic",
                        note = "Where servers are kept: as the desktop is."
                    ),
                ),
                // Names.
                ("skwir", Words::as_written("Skwir")),
                ("kde", Words::as_written("KDE")),
            ],
        )
        .now(if on_kde(&skwir_platform::apps::current_desktops()) {
            "kde"
        } else {
            "skwir"
        })
        .page(skwir_core::settings::page::NETWORK)
        .section(skwir_core::settings::section::SERVERS),
    );
    app.settings.declare(SettingSpec::data(
        LIST,
        words!(
            remote_servers_list = "The servers added by their address that Skwir keeps itself: each one's name and address"
        ),
        toml::Value::Array(Vec::new()),
    ));
}

/// Where a server of `scheme` added now is kept, as the setting says and
/// the desktop is.
pub fn keeper(app: &App, scheme: Scheme) -> Kept {
    let chosen: String = app
        .settings
        .get(KEPT_IN)
        .unwrap_or_else(|| "automatic".to_owned());
    keeper_on(
        &chosen,
        &skwir_platform::apps::current_desktops(),
        crate::kio::opens(scheme),
    )
}

/// [`keeper`] for `chosen`, on `desktops`, of a kind KIO opens or not.
fn keeper_on(chosen: &str, desktops: &[String], kio_opens: bool) -> Kept {
    let kde = chosen == "kde" || (chosen == "automatic" && on_kde(desktops));
    if kde && kio_opens {
        Kept::Kde
    } else {
        Kept::Skwir
    }
}

/// Whether `desktops` (`XDG_CURRENT_DESKTOP`'s) are KDE's: where
/// Automatic keeps servers in KDE's network folders.
fn on_kde(desktops: &[String]) -> bool {
    desktops.iter().any(|d| d.eq_ignore_ascii_case("KDE"))
}

/// Every server kept: Skwir's list, then KDE's network folders, each
/// named as it was kept.
pub fn listed(app: &App) -> Vec<Server> {
    let mut servers: Vec<Server> = own(app)
        .into_iter()
        .filter_map(|entry| {
            Some(server(
                &entry.name,
                Address::parse(&entry.address).ok()?,
                Kept::Skwir,
            ))
        })
        .collect();
    for (_, kde) in kde_folder()
        .map(|f| network_folders(&f))
        .unwrap_or_default()
    {
        if !servers.iter().any(|s| s.address == kde.address) {
            servers.push(kde);
        }
    }
    servers
}

/// The server kept at exactly `address`, if one is.
pub fn find(app: &App, address: &Address) -> Option<Server> {
    listed(app).into_iter().find(|s| s.address == *address)
}

/// Keep the server at `address` under `name`, where [`keeper`] says: empty,
/// it goes by its address's (`Address::label`), said where it is shown in
/// Skwir's list, in English in KDE's network folders, which other programs
/// read.
///
/// # Errors
/// When a server is kept at that address already, or it could not be
/// written.
pub fn add(app: &App, name: &str, address: &Address) -> anyhow::Result<Kept> {
    if let Some(kept) = find(app, address) {
        anyhow::bail!(words!(
            remote_servers_taken = "{ $name } is a server already, at that address",
            name = crate::address::named(&kept.label),
            note = "name: the server's, as the Network section shows it."
        ));
    }
    let english;
    let named = if name.is_empty() {
        english = address.label().english();
        english.as_str()
    } else {
        name
    };
    let kept = keeper(app, address.scheme);
    match kept {
        Kept::Kde => {
            let folder = kde_folder().ok_or_else(|| {
                anyhow::anyhow!(words!(
                    remote_servers_no_kde_folder =
                        "there is no data folder for KDE's network folders"
                ))
            })?;
            std::fs::create_dir_all(&folder)?;
            write_entry(&free_name(&folder, named), named, address)?;
            tell_kde();
        }
        Kept::Skwir | Kept::Tool(_) => {
            let mut all = own(app);
            all.push(Entry {
                name: name.to_owned(),
                address: address.to_string(),
            });
            keep_own(app, &all)?;
        }
    }
    Ok(kept)
}

/// The server kept at `old` named `name` (empty: its address's, as `add`
/// keeps it) and at `new` from now on, where it is kept.
///
/// # Errors
/// When no server is kept at `old`, another is at `new`, or it could not
/// be written.
pub fn change(app: &App, old: &Address, name: &str, new: &Address) -> anyhow::Result<()> {
    if new != old && find(app, new).is_some() {
        anyhow::bail!(words!(
            remote_servers_other_there = "another server is kept at that address"
        ));
    }
    let mut all = own(app);
    if let Some(entry) = all
        .iter_mut()
        .find(|e| Address::parse(&e.address).is_ok_and(|a| a == *old))
    {
        name.clone_into(&mut entry.name);
        entry.address = new.to_string();
        return keep_own(app, &all);
    }
    let file = kde_file_of(old).ok_or_else(|| anyhow::anyhow!(none_kept(old)))?;
    if name.is_empty() {
        write_entry(&file, &new.label().english(), new)?;
    } else {
        write_entry(&file, name, new)?;
    }
    tell_kde();
    Ok(())
}

/// Forget the server kept at `address`, where it is kept.
///
/// # Errors
/// When none is kept there, or it could not be written.
pub fn remove(app: &App, address: &Address) -> anyhow::Result<()> {
    let mut all = own(app);
    let before = all.len();
    all.retain(|e| Address::parse(&e.address).map_or(true, |a| a != *address));
    if all.len() != before {
        return keep_own(app, &all);
    }
    let file = kde_file_of(address).ok_or_else(|| anyhow::anyhow!(none_kept(address)))?;
    std::fs::remove_file(&file)?;
    tell_kde();
    Ok(())
}

/// No server is kept at `address`, said.
fn none_kept(address: &Address) -> Words {
    words!(
        remote_servers_none_kept = "no server is kept at { $address }",
        address = address.to_string(),
        note = "address: a server's address (sftp://me@example.com)."
    )
}

fn own(app: &App) -> Vec<Entry> {
    app.settings.get(LIST).unwrap_or_default()
}

fn keep_own(app: &App, all: &[Entry]) -> anyhow::Result<()> {
    app.settings.set(LIST, toml::Value::try_from(all)?);
    app.settings.save()
}

/// What a server kept under `name` is called: that name, as written, else
/// (empty) its address's.
pub fn label_of(name: &str, address: &Address) -> Words {
    if name.is_empty() {
        address.label()
    } else {
        Words::as_written(name.to_owned())
    }
}

fn server(label: &str, address: Address, kept: Kept) -> Server {
    Server {
        label: label_of(label, &address),
        address,
        kind: String::new(),
        kept,
    }
}

/// KDE's network folders: `$XDG_DATA_HOME/remoteview`.
fn kde_folder() -> Option<PathBuf> {
    skwir_core::xdg::data_home().map(|data| data.join("remoteview"))
}

/// Where the servers kept by their address are read, followed
/// (`connector::Source`): KDE's network folders, which Dolphin's Add
/// Network Folder writes too. Skwir's own list is the plugin's data,
/// changed through Skwir.
pub fn sources() -> Vec<Source> {
    kde_folder().map(Source::Folder).into_iter().collect()
}

/// What KDE's programs are told once its network folders changed:
/// `KDirNotify`'s `FilesAdded` of `remote:/`, as KDE's own Add Network
/// Folder (`knetattach`) says it, and KIO's watcher of the folder
/// (`remotedirnotify`, in `kded6`, not running outside Plasma) says it
/// for any change, a removal too. An open Dolphin showing Network lists
/// it again. Sent on `bus`.
///
/// # Errors
/// When the bus refused the signal.
#[cfg(target_os = "linux")]
pub fn tell_kde_on(bus: &zbus::blocking::Connection) -> zbus::Result<()> {
    bus.emit_signal(
        None::<&str>,
        "/",
        "org.kde.KDirNotify",
        "FilesAdded",
        &("remote:/",),
    )
}

/// [`tell_kde_on`] the session's bus, on a thread of its own: never
/// waited for, and nothing said where no session bus is.
fn tell_kde() {
    #[cfg(target_os = "linux")]
    {
        let told = std::thread::Builder::new()
            .name("skwir-tell-kde".into())
            .spawn(|| {
                if let Err(error) = skwir_platform::bus::session().and_then(|bus| tell_kde_on(&bus))
                {
                    tracing::debug!("KDE was not told its network folders changed: {error}");
                }
            });
        if let Err(error) = told {
            tracing::debug!("KDE was not told its network folders changed: {error}");
        }
    }
}

/// The desktop entry of KDE's network folders that links to `address`.
fn kde_file_of(address: &Address) -> Option<PathBuf> {
    network_folders(&kde_folder()?)
        .into_iter()
        .find(|(_, s)| s.address == *address)
        .map(|(file, _)| file)
}

/// KDE's network folders (`remote:/`), each a desktop entry in `folder`
/// linking to its address: those of a protocol Skwir opens, not hidden,
/// named as the entry names them, in the order of their files' names.
pub fn network_folders(folder: &Path) -> Vec<(PathBuf, Server)> {
    let Ok(entries) = std::fs::read_dir(folder) else {
        return Vec::new();
    };
    let mut files: Vec<PathBuf> = entries
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|e| e == "desktop"))
        .collect();
    files.sort();
    files
        .into_iter()
        .filter_map(|file| {
            // A link is a few lines: one longer is no link.
            let text = skwir_core::fs::file::read_regular(&file, 64 * 1024).ok()?;
            let server = network_folder(&String::from_utf8_lossy(&text))?;
            Some((file, server))
        })
        .collect()
}

/// The network folder a desktop entry of KDE's `remoteview` links to.
fn network_folder(entry: &str) -> Option<Server> {
    let mut in_entry = false;
    let (mut name, mut url, mut hidden) = (None, None, false);
    for line in entry.lines().map(str::trim_start) {
        if line.starts_with('[') {
            in_entry = line.trim_end() == "[Desktop Entry]";
            continue;
        }
        if !in_entry {
            continue;
        }
        // Spaces around `=` are no part of the value: one it starts with
        // is written `\s`.
        match line.split_once('=').map(|(k, v)| (k.trim(), v.trim())) {
            Some(("Name", value)) => name = Some(unescaped(value)),
            Some(("URL", value)) => url = Some(unescaped(value)),
            Some(("Hidden", value)) => hidden = value == "true",
            _ => {}
        }
    }
    if hidden {
        return None;
    }
    let address = Address::parse(&url?).ok()?;
    Some(server(&name.unwrap_or_default(), address, Kept::Kde))
}

/// Write the desktop entry `file` of KDE's network folders, linking to
/// `address` under `name`, as KDE's Add Network Folder writes one: a file
/// KDE shares, replaced whole (`file::rewrite_words`).
fn write_entry(file: &Path, name: &str, address: &Address) -> anyhow::Result<()> {
    let url = crate::kio::kio_url(address).ok_or_else(|| {
        anyhow::anyhow!(words!(
            remote_servers_kio_opens_no = "KIO opens no { $protocol } address",
            protocol = address.scheme.title(),
            note = "protocol: SFTP, SMB, WebDAV…"
        ))
    })?;
    let text = format!(
        "[Desktop Entry]\nCharset=\nIcon=folder-remote\nName={}\nType=Link\nURL={}\n",
        escaped(name),
        escaped(&url)
    );
    let read_only = words!(
        remote_servers_kde_read_only =
            "KDE's network folder is read-only; change it where it is managed",
        note = "Why a server was not added to KDE's network folders: its file is a link to one that cannot be written, as a configuration managed elsewhere (Nix's Home Manager) makes it."
    );
    skwir_platform::file::rewrite_words(file, read_only, |_| Some((text, ())))?;
    Ok(())
}

/// A file of `folder` no entry has yet, named after `name`, ` 2`, ` 3`
/// after a name taken: a name this system takes (`op::name_from`, `_` for
/// what it refuses), and one KDE lists and opens. KDE lists its network
/// folders' `*.desktop` but its hidden files (a name begun with a dot,
/// which takes `server` before it), and opens each as `remote:/` and its
/// name joined as text, which it reads back as a URL (KIO's
/// `RemoteImpl::createEntry`): `?` and `#` would end its path there, and
/// `%` begin an escape, so they are `_` too, on every system.
fn free_name(folder: &Path, name: &str) -> PathBuf {
    let cut: String = name.chars().take(80).collect();
    let cut = cut.trim();
    let base = if cut.is_empty() || cut.starts_with('.') {
        format!("server{cut}")
    } else {
        cut.to_owned()
    };
    let base: String = skwir_core::op::name_from(&base, '_')
        .chars()
        .map(|c| if matches!(c, '?' | '#' | '%') { '_' } else { c })
        .collect();
    (1..=1000)
        .map(|n| {
            if n == 1 {
                folder.join(format!("{base}.desktop"))
            } else {
                folder.join(format!("{base} {n}.desktop"))
            }
        })
        .find(|file| !file.exists())
        .unwrap_or_else(|| folder.join("server.desktop"))
}

/// A desktop entry's value as the specification escapes it: `\\`, `\n`,
/// `\t`, `\r`, a leading space as `\s`.
fn escaped(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    for (at, c) in value.chars().enumerate() {
        match c {
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\t' => out.push_str("\\t"),
            '\r' => out.push_str("\\r"),
            ' ' if at == 0 => out.push_str("\\s"),
            c => out.push(c),
        }
    }
    out
}

/// A desktop entry's value read back ([`escaped`]'s way).
fn unescaped(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    let mut chars = value.chars();
    while let Some(c) = chars.next() {
        if c != '\\' {
            out.push(c);
            continue;
        }
        match chars.next() {
            Some('s') => out.push(' '),
            Some('n') => out.push('\n'),
            Some('t') => out.push('\t'),
            Some('r') => out.push('\r'),
            Some('\\') | None => out.push('\\'),
            Some(other) => {
                out.push('\\');
                out.push(other);
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// KDE's network folders, each a desktop entry linking to its address,
    /// are servers named as the entries name them.
    #[test]
    fn kde_s_network_folders_are_servers() {
        let folder = tempfile::tempdir().unwrap();
        std::fs::write(
            folder.path().join("b-photos.desktop"),
            "[Desktop Entry]\nCharset=\nIcon=folder-remote\nName=It's the photos\nType=Link\nURL=smb://WORK;me@nas/photos\n",
        )
        .unwrap();
        std::fs::write(
            folder.path().join("a-home.desktop"),
            "[Desktop Entry]\nName=\nType=Link\nURL=sftp://me@example.com:2222/home/me\n",
        )
        .unwrap();
        std::fs::write(
            folder.path().join("c-gone.desktop"),
            "[Desktop Entry]\nName=Gone\nHidden=true\nURL=ftp://old\n",
        )
        .unwrap();
        std::fs::write(
            folder.path().join("d-other.desktop"),
            "[Desktop Entry]\nName=Not a server\nURL=file:///home\n",
        )
        .unwrap();
        std::fs::write(folder.path().join("notes.txt"), "URL=ftp://x\n").unwrap();
        let said: Vec<(String, String, Kept)> = network_folders(folder.path())
            .into_iter()
            .map(|(_, s)| (s.label.english(), s.address.to_string(), s.kept))
            .collect();
        assert_eq!(
            said,
            [
                (
                    "me@example.com:2222".to_owned(),
                    "sftp://me@example.com:2222/home/me".to_owned(),
                    Kept::Kde
                ),
                (
                    "It's the photos".to_owned(),
                    "smb://WORK;me@nas/photos".to_owned(),
                    Kept::Kde
                ),
            ]
        );
        assert!(network_folders(&folder.path().join("none")).is_empty());
    }

    /// An entry Skwir writes is one KDE reads (WebDAV as `webdav://`), its
    /// name escaped as the specification says and read back whole.
    #[test]
    fn a_network_folder_written_reads_back() {
        let folder = tempfile::tempdir().unwrap();
        let name = " Ünïcode \\ it's\nnew";
        let address = Address::parse("dav://me@nas.local:8080/dav/it's shared").unwrap();
        let file = free_name(folder.path(), name);
        assert_eq!(
            file.file_name().unwrap().to_str().unwrap(),
            if cfg!(windows) {
                "Ünïcode _ it's_new.desktop"
            } else {
                "Ünïcode \\ it's\nnew.desktop"
            },
            "what this system refuses in a name, `_`"
        );
        write_entry(&file, name, &address).unwrap();
        let text = std::fs::read_to_string(&file).unwrap();
        assert!(
            text.contains("Name=\\s\u{dc}n\u{ef}code \\\\ it's\\nnew\n"),
            "{text}"
        );
        assert!(
            text.contains("URL=webdav://me@nas.local:8080/dav/it's%20shared\n"),
            "{text}"
        );
        let read = network_folders(folder.path());
        assert_eq!(read.len(), 1);
        assert_eq!(read[0].1.label.english(), name);
        assert_eq!(read[0].1.address, address);
        // A second of the same name is a file of its own.
        std::fs::write(free_name(folder.path(), name), "").unwrap();
        assert!(
            free_name(folder.path(), name)
                .to_str()
                .unwrap()
                .ends_with(" 3.desktop")
        );
        assert_eq!(
            free_name(folder.path(), ".hidden")
                .file_name()
                .unwrap()
                .to_str()
                .unwrap(),
            "server.hidden.desktop"
        );
        let named = |name: &str| {
            free_name(folder.path(), name)
                .file_name()
                .unwrap()
                .to_str()
                .unwrap()
                .to_owned()
        };
        // Windows refuses `:` (and a device's name); KDE's `remote:/` reads
        // `?`, `#` and `%` as a URL's.
        assert_eq!(
            named("NAS: photos"),
            if cfg!(windows) {
                "NAS_ photos.desktop"
            } else {
                "NAS: photos.desktop"
            }
        );
        assert_eq!(named("nas #2? 100%41"), "nas _2_ 100_41.desktop");
        assert_eq!(
            named("con"),
            if cfg!(windows) {
                "_con.desktop"
            } else {
                "con.desktop"
            }
        );
        assert_eq!(named("  "), "server.desktop");
        assert_eq!(named("a/b"), "a_b.desktop");
    }

    #[test]
    fn values_escaped_read_back() {
        for value in ["plain", " lead", "a\\b", "line\nnext\tand\r", "\\s kept"] {
            assert_eq!(unescaped(&escaped(value)), value);
        }
        assert_eq!(unescaped("a\\qb\\"), "a\\qb\\");
    }

    /// Automatic keeps them in KDE's network folders on KDE's desktop,
    /// where KIO opens the kind, in Skwir's list elsewhere.
    #[test]
    fn where_a_server_is_kept() {
        let kde = ["KDE".to_owned()];
        let gnome = ["GNOME".to_owned()];
        assert_eq!(keeper_on("automatic", &kde, true), Kept::Kde);
        assert_eq!(keeper_on("automatic", &kde, false), Kept::Skwir);
        assert_eq!(keeper_on("automatic", &gnome, true), Kept::Skwir);
        assert_eq!(keeper_on("kde", &gnome, true), Kept::Kde);
        assert_eq!(keeper_on("kde", &gnome, false), Kept::Skwir);
        assert_eq!(keeper_on("skwir", &kde, true), Kept::Skwir);
    }
}
