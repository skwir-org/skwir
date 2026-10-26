//! KDE's kmtpd as this helper asks it, on the session bus (ADR-097): its
//! devices (`org.kde.kmtp.Daemon`, `org.kde.kmtp.Device`) and their
//! storages (`org.kde.kmtp.Storage`), by a storage's own paths
//! (`/DCIM/Camera/a.jpg`, `/` its top). kmtpd answers one call at a time,
//! on `kiod6`'s one thread: so does this client. Its copies are started by
//! a call and end with a `copyFinished` signal, whoever asked for them.

use std::fs::File;
use std::io;
use std::os::fd::AsFd;

use zbus::blocking::{Connection, MessageIterator, Proxy};
use zbus::zvariant::{Fd, OwnedObjectPath};

/// `kiod6`, which hosts kmtpd.
pub const SERVICE: &str = "org.kde.kiod6";
const DAEMON_PATH: &str = "/modules/kmtpd";
const DAEMON: &str = "org.kde.kmtp.Daemon";
const DEVICE: &str = "org.kde.kmtp.Device";
const STORAGE: &str = "org.kde.kmtp.Storage";

/// An entry as kmtpd lists one: its object, its parent's, its storage's,
/// its name, size, time of change (seconds since 1970) and type
/// (`inode/directory` for a folder).
pub type Listed = (u32, u32, u32, String, u64, i64, String);

/// Whether `entry` is a folder.
pub fn is_folder(entry: &Listed) -> bool {
    entry.6 == "inode/directory"
}

/// A storage of the device: its object and what the phone calls it.
#[derive(Clone, Debug)]
pub struct Storage {
    pub path: OwnedObjectPath,
    pub description: String,
}

/// The session bus as this helper asks kmtpd: no call given up on by
/// time, as kmtpd answers once the phone does (a locked phone keeps a
/// listing a minute; a large folder lists in tens of seconds).
pub fn connect() -> io::Result<Connection> {
    zbus::blocking::connection::Builder::session()
        .and_then(zbus::blocking::connection::Builder::build)
        .map_err(failed)
}

/// What a call's failure is to the file system.
pub fn failed(error: zbus::Error) -> io::Error {
    io::Error::other(error)
}

/// kmtpd's object at `path` speaking `interface`, its properties asked
/// each time: never all at once, as a storage's free space waits for the
/// phone.
fn proxy<'a>(bus: &'a Connection, path: &'a str, interface: &'static str) -> io::Result<Proxy<'a>> {
    zbus::blocking::proxy::Builder::<Proxy<'a>>::new(bus)
        .destination(SERVICE)
        .and_then(|builder| builder.path(path))
        .and_then(|builder| builder.interface(interface))
        .map(|builder| builder.cache_properties(zbus::proxy::CacheProperties::No))
        .and_then(zbus::blocking::proxy::Builder::build)
        .map_err(failed)
}

/// The device kmtpd knows by `udi`, if it holds it.
pub fn device(bus: &Connection, udi: &str) -> io::Result<Option<OwnedObjectPath>> {
    let daemon = proxy(bus, DAEMON_PATH, DAEMON)?;
    let devices: Vec<OwnedObjectPath> = daemon.call("listDevices", &()).map_err(failed)?;
    for device in devices {
        let known: String = proxy(bus, device.as_str(), DEVICE)?
            .get_property("udi")
            .map_err(failed)?;
        if known == udi {
            return Ok(Some(device));
        }
    }
    Ok(None)
}

/// The storages of `device`, each by its description.
pub fn storages(bus: &Connection, device: &OwnedObjectPath) -> io::Result<Vec<Storage>> {
    let proxy = proxy(bus, device.as_str(), DEVICE)?;
    let paths: Vec<OwnedObjectPath> = proxy.call("listStorages", &()).map_err(failed)?;
    paths
        .into_iter()
        .map(|path| {
            let description: String = storage(bus, &path)?
                .get_property("description")
                .map_err(failed)?;
            Ok(Storage { path, description })
        })
        .collect()
}

fn storage<'a>(bus: &'a Connection, path: &'a OwnedObjectPath) -> io::Result<Proxy<'a>> {
    proxy(bus, path.as_str(), STORAGE)
}

/// The folder at `path` of `storage`: its entries, or why not (`NotFound`,
/// `NotADirectory`).
pub fn list(bus: &Connection, storage: &Storage, path: &str) -> io::Result<Vec<Listed>> {
    let (listed, result): (Vec<Listed>, i32) = self::storage(bus, &storage.path)?
        .call("getFilesAndFolders", &(path,))
        .map_err(failed)?;
    match result {
        0 => Ok(listed),
        1 => Err(io::ErrorKind::NotFound.into()),
        _ => Err(io::ErrorKind::NotADirectory.into()),
    }
}

/// What `storage` knows of `path`; `None` where nothing is.
pub fn metadata(bus: &Connection, storage: &Storage, path: &str) -> io::Result<Option<Listed>> {
    let entry: Listed = self::storage(bus, &storage.path)?
        .call("getFileMetadata", &(path,))
        .map_err(failed)?;
    Ok((entry.0 != 0).then_some(entry))
}

/// The whole file at `path` copied into `into` (empty), `size` bytes long.
/// kmtpd starts the copy, then says it ended (`copyFinished`): one of
/// another program's ending first leaves `into` short, and the wait goes on
/// for this one.
pub fn fetch(
    bus: &Connection,
    storage: &Storage,
    path: &str,
    into: &File,
    size: u64,
) -> io::Result<()> {
    let ended = copies_ended(bus, storage)?;
    let started: i32 = self::storage(bus, &storage.path)?
        .call("getFileToFileDescriptor", &(Fd::from(into.as_fd()), path))
        .map_err(failed)?;
    if started != 0 {
        return Err(io::ErrorKind::NotFound.into());
    }
    wait_for(ended, || Ok(into.metadata()?.len() >= size))
}

/// The file `from` sent to `path` of `storage`, a new entry (MTP replaces
/// nothing: what was there is removed first by the caller). Ended once
/// kmtpd says so and the phone lists it, `size` bytes long.
pub fn send(
    bus: &Connection,
    storage: &Storage,
    from: &File,
    path: &str,
    size: u64,
) -> io::Result<()> {
    let ended = copies_ended(bus, storage)?;
    let started: i32 = self::storage(bus, &storage.path)?
        .call(
            "sendFileFromFileDescriptor",
            &(Fd::from(from.as_fd()), path),
        )
        .map_err(failed)?;
    if started != 0 {
        return Err(io::Error::other(
            "the phone refused it (no such folder there)",
        ));
    }
    wait_for(ended, || {
        Ok(metadata(bus, storage, path)?.is_some_and(|entry| entry.4 == size))
    })
}

/// kmtpd's copies of `storage` ending, listened to before one is started.
fn copies_ended(bus: &Connection, storage: &Storage) -> io::Result<MessageIterator> {
    let rule = zbus::MatchRule::builder()
        .msg_type(zbus::message::Type::Signal)
        .path(storage.path.as_str())
        .and_then(|rule| rule.interface(STORAGE))
        .and_then(|rule| rule.member("copyFinished"))
        .map_err(failed)?
        .build();
    MessageIterator::for_match_rule(rule, bus, None).map_err(failed)
}

/// Wait for a copy's end that leaves `done` true; one that ends badly
/// with `done` still false is this copy's failure.
fn wait_for(ended: MessageIterator, mut done: impl FnMut() -> io::Result<bool>) -> io::Result<()> {
    for message in ended {
        let message = message.map_err(failed)?;
        let result: i32 = message.body().deserialize().map_err(failed)?;
        if done()? {
            return Ok(());
        }
        if result != 0 {
            return Err(io::Error::other(format!(
                "the phone ended the copy (MTP error {result})"
            )));
        }
    }
    Err(io::Error::other("kmtpd left the bus"))
}

/// Rename the entry at `path` to `name`, in its folder.
pub fn rename(bus: &Connection, storage: &Storage, path: &str, name: &str) -> io::Result<()> {
    let result: i32 = self::storage(bus, &storage.path)?
        .call("setFileName", &(path, name))
        .map_err(failed)?;
    refused(result, "renaming it")
}

/// Make the folder `path`.
pub fn make_folder(bus: &Connection, storage: &Storage, path: &str) -> io::Result<()> {
    let made: u32 = self::storage(bus, &storage.path)?
        .call("createFolder", &(path,))
        .map_err(failed)?;
    if made == 0 {
        return Err(io::Error::other("the phone refused to make the folder"));
    }
    Ok(())
}

/// Remove the entry at `path`.
pub fn delete(bus: &Connection, storage: &Storage, path: &str) -> io::Result<()> {
    let result: i32 = self::storage(bus, &storage.path)?
        .call("deleteObject", &(path,))
        .map_err(failed)?;
    refused(result, "removing it")
}

fn refused(result: i32, doing: &str) -> io::Result<()> {
    match result {
        0 => Ok(()),
        1 => Err(io::ErrorKind::NotFound.into()),
        _ => Err(io::Error::other(format!(
            "the phone refused {doing} (MTP error {result})"
        ))),
    }
}

/// The storage's whole and free bytes.
pub fn space(bus: &Connection, storage: &Storage) -> io::Result<(u64, u64)> {
    let proxy = self::storage(bus, &storage.path)?;
    let whole: u64 = proxy.get_property("maxCapacity").map_err(failed)?;
    let free: u64 = proxy.get_property("freeSpaceInBytes").map_err(failed)?;
    Ok((whole, free))
}
