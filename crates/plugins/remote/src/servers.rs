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

use crate::address::{Address, Scheme};
use crate::connector::Server;

/// Skwir's own list of servers: each one's name and address.
pub const LIST: &str = "remote.kept_servers";

/// Where a server added by its address is kept.
pub const KEPT_IN: &str = "remote.servers_kept_in";

/// Who keeps a server the Network section lists.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Kept {
    /// The tool that reads it, which changes it, where the words say:
    /// "in rclone's configuration", "one of Dolphin's places".
    Tool(&'static str),
    /// Skwir's own list.
    Skwir,
    /// KDE's network folders.
    Kde,
}

impl Kept {
    /// Where it is kept, in the user's words: "in Skwir's list".
    pub fn said(self) -> &'static str {
        match self {
            Kept::Tool(words) => words,
            Kept::Skwir => "in Skwir's list",
            Kept::Kde => "in KDE's network folders",
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
        SettingSpec::choice(
            KEPT_IN,
            "Where servers are kept",
            "Where a server added by its address is kept. Automatic: in KDE's network folders on KDE's desktop, where Dolphin lists them too, and in Skwir's own list elsewhere; a kind KIO does not open (FTPS, AFP) always in Skwir's.",
            "automatic",
            &[
                ("automatic", "Automatic"),
                ("skwir", "Skwir"),
                ("kde", "KDE"),
            ],
        )
        .page(skwir_core::settings::page::NETWORK)
        .section("Servers"),
    );
    app.settings.declare(SettingSpec::data(
        LIST,
        "The servers added by their address that Skwir keeps itself: each one's name and address",
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
    let on_kde = desktops.iter().any(|d| d.eq_ignore_ascii_case("KDE"));
    let kde = chosen == "kde" || (chosen == "automatic" && on_kde);
    if kde && kio_opens {
        Kept::Kde
    } else {
        Kept::Skwir
    }
}

/// Every server kept: Skwir's list, then KDE's network folders, each
/// named as it was kept.
pub fn listed(app: &App) -> Vec<Server> {
    let mut servers: Vec<Server> = own(app)
        .into_iter()
        .filter_map(|entry| {
            Some(server(
                entry.name,
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

/// Keep the server at `address` under `name`, where [`keeper`] says.
///
/// # Errors
/// When a server is kept at that address already, or it could not be
/// written.
pub fn add(app: &App, name: &str, address: &Address) -> anyhow::Result<Kept> {
    if let Some(kept) = find(app, address) {
        anyhow::bail!("“{}” is a server already, at that address", kept.label);
    }
    let kept = keeper(app, address.scheme);
    match kept {
        Kept::Kde => {
            let folder = kde_folder().ok_or_else(|| {
                anyhow::anyhow!("there is no data folder for KDE's network folders")
            })?;
            std::fs::create_dir_all(&folder)?;
            write_entry(&free_name(&folder, name), name, address)?;
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

/// The server kept at `old` named `name` and at `new` from now on, where it
/// is kept.
///
/// # Errors
/// When no server is kept at `old`, another is at `new`, or it could not
/// be written.
pub fn change(app: &App, old: &Address, name: &str, new: &Address) -> anyhow::Result<()> {
    if new != old && find(app, new).is_some() {
        anyhow::bail!("another server is kept at that address");
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
    let file = kde_file_of(old).ok_or_else(|| anyhow::anyhow!("no server is kept at {old}"))?;
    write_entry(&file, name, new)
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
    let file =
        kde_file_of(address).ok_or_else(|| anyhow::anyhow!("no server is kept at {address}"))?;
    std::fs::remove_file(&file)?;
    Ok(())
}

fn own(app: &App) -> Vec<Entry> {
    app.settings.get(LIST).unwrap_or_default()
}

fn keep_own(app: &App, all: &[Entry]) -> anyhow::Result<()> {
    app.settings.set(LIST, toml::Value::try_from(all)?);
    app.settings.save()
}

fn server(label: String, address: Address, kept: Kept) -> Server {
    Server {
        label: if label.is_empty() {
            address.label()
        } else {
            label
        },
        address,
        kind: String::new(),
        kept,
    }
}

/// KDE's network folders: `$XDG_DATA_HOME/remoteview`.
fn kde_folder() -> Option<PathBuf> {
    skwir_core::xdg::data_home().map(|data| data.join("remoteview"))
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
    Some(server(name.unwrap_or_default(), address, Kept::Kde))
}

/// Write the desktop entry `file` of KDE's network folders, linking to
/// `address` under `name`, as KDE's Add Network Folder writes one: a file
/// KDE shares, replaced whole (`file::rewrite`).
fn write_entry(file: &Path, name: &str, address: &Address) -> anyhow::Result<()> {
    let url = crate::kio::kio_url(address)
        .ok_or_else(|| anyhow::anyhow!("KIO opens no {} address", address.scheme.title()))?;
    let text = format!(
        "[Desktop Entry]\nCharset=\nIcon=folder-remote\nName={}\nType=Link\nURL={}\n",
        escaped(name),
        escaped(&url)
    );
    skwir_platform::file::rewrite(file, "KDE's network folder", |_| Some((text, ())))?;
    Ok(())
}

/// A file of `folder` no entry has yet, named after `name`: what a file's
/// name cannot hold made `_`, ` 2`, ` 3` after a name taken.
fn free_name(folder: &Path, name: &str) -> PathBuf {
    let base: String = name
        .chars()
        .map(|c| {
            if c == '/' || c == '\\' || c.is_control() {
                '_'
            } else {
                c
            }
        })
        .take(80)
        .collect();
    let base = base.trim();
    let base = if base.is_empty() || base.starts_with('.') {
        format!("server{base}")
    } else {
        base.to_owned()
    };
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
            .map(|(_, s)| (s.label, s.address.to_string(), s.kept))
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
            "Ünïcode \\ it's_new.desktop".replace('\\', "_")
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
        assert_eq!(read[0].1.label, name);
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
