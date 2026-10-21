//! What a connector is (ADR-077): an installed tool that turns an address
//! into a mounted folder, lets go of it, and knows its own mounts. Adding
//! a protocol is a scheme a connector maps, or a connector.

use std::path::PathBuf;
use std::sync::Arc;

use skwir_core::cancel::CancelToken;
use skwir_core::fs::reach::Remote;

use crate::address::{Address, Scheme};

/// What a connection needs only the user can say, asked of the window
/// (`Window`) or, in tests, answered beforehand. Each call waits for the
/// answer; `None` and `false` are Cancel, and so is `cancel` firing.
pub trait Ask: Send + Sync {
    /// A secret: a password, a key's passphrase. `title` says whose
    /// ("Password for me@example.com"), `accept` the button's verb.
    fn secret(&self, title: &str, accept: &str, cancel: &CancelToken) -> Option<String>;
    /// Whether to go on where the server cannot be vouched for (a host
    /// key never seen, a certificate not trusted): `detail` is what the
    /// tool said, fingerprint included.
    fn confirm(&self, title: &str, detail: &str, cancel: &CancelToken) -> bool;
}

/// A folder a connector mounted, or found mounted.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Connected {
    /// Where the mount's top is.
    pub point: PathBuf,
    /// The address of that top.
    pub address: Address,
}

/// A mount of the table this connector recognises as its tool's.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Recognised {
    /// The address of its top, when it has one Skwir opens.
    pub address: Option<Address>,
    /// What the user calls it.
    pub label: String,
    /// Skwir made it (under its runtime folder): letting go of it removes
    /// its mount point too.
    pub ours: bool,
    /// A device on this machine rather than a server: a phone or a camera
    /// GVfs reads over USB. It is near, whatever its address, and among
    /// the sidebar's devices.
    pub device: Option<Device>,
}

/// A device GVfs reads on this machine.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Device {
    Phone,
    Camera,
}

impl Device {
    /// Its theme icon.
    pub fn icon(self) -> &'static str {
        match self {
            Device::Phone => "phone",
            Device::Camera => "camera-photo",
        }
    }
}

/// A server the user can connect to, from the tool's configuration: an
/// rclone remote, a network entry of fstab.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Server {
    pub address: Address,
    pub label: String,
}

/// An installed tool that mounts remotes.
pub trait Connector: Send + Sync {
    /// Its name in settings and messages: `sshfs`, `gvfs`, `rclone`.
    fn id(&self) -> &'static str;
    /// Its name for the user: "sshfs", "GVfs".
    fn title(&self) -> &'static str;
    /// The schemes it opens.
    fn schemes(&self) -> &'static [Scheme];
    /// Why it cannot connect here, in the user's words ("sshfs is not
    /// installed"); `None` when it can. Cheap: a lookup on `PATH`.
    fn unavailable(&self) -> Option<String>;
    /// Mount `address`'s top (`Address::top`) and say where. Blocks: on a
    /// worker, asking what it must through `ask`. Stops when `cancel`
    /// fires, leaving nothing mounted.
    ///
    /// # Errors
    /// What the tool said, in the user's words where it can be told.
    fn connect(
        &self,
        address: &Address,
        ask: &Arc<dyn Ask>,
        cancel: &CancelToken,
    ) -> anyhow::Result<Connected>;
    /// Whether `remote`, a mount of the table, is this tool's, and of what.
    fn recognises(&self, remote: &Remote) -> Option<Recognised>;
    /// Let go of `remote`, one it recognises. Blocks: on a worker.
    ///
    /// # Errors
    /// When the tool refused (busy, gone).
    fn disconnect(
        &self,
        remote: &Remote,
        recognised: &Recognised,
        cancel: &CancelToken,
    ) -> anyhow::Result<()>;
    /// The servers its configuration names. Reads files and may start the
    /// tool: on a worker.
    fn servers(&self, _cancel: &CancelToken) -> Vec<Server> {
        Vec::new()
    }
    /// What copies many files at once between this machine and a mount it
    /// made (ADR-078), where its tool has a way: sshfs's, rsync through
    /// its connection.
    fn transfer(&self) -> Option<std::sync::Arc<dyn skwir_core::op::transfer::Transfer>> {
        None
    }
}

/// Where Skwir mounts what it connects itself (sshfs, rclone): a folder
/// of its own under the user's runtime folder, one per address, named so
/// that the address can be read back from it after Skwir restarts
/// (principle 3: nothing kept but the mount itself).
pub fn mounts_folder() -> Option<PathBuf> {
    #[cfg(unix)]
    {
        skwir_core::xdg::runtime_dir().map(|dir| dir.join("skwir").join("remote"))
    }
    #[cfg(not(unix))]
    {
        skwir_core::xdg::cache_home().map(|dir| dir.join("skwir").join("remote"))
    }
}

/// The mount point's name for `address` mounted by `connector`:
/// `sshfs sftp:me@example.com:2222`, readable back by [`of_point_name`].
pub fn point_name(connector: &str, address: &Address) -> String {
    let text = address.top().to_string();
    // A folder's name holds no `/`: written as GVfs writes its own, `%2F`.
    let text = text.replace('%', "%25").replace('/', "%2F");
    format!("{connector} {text}")
}

/// The connector and address a mount point's name says ([`point_name`]).
pub fn of_point_name(name: &str) -> Option<(&str, Address)> {
    let (connector, text) = name.split_once(' ')?;
    let text = String::from_utf8(crate::address::decoded(text)).ok()?;
    Some((connector, Address::parse(&text).ok()?))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_mount_point_says_its_address() {
        let address = Address::parse("sftp://me@example.com:2222/home/me").unwrap();
        let name = point_name("sshfs", &address);
        assert!(!name.contains('/'), "{name}");
        let (connector, back) = of_point_name(&name).unwrap();
        assert_eq!(connector, "sshfs");
        assert_eq!(back, address.top());
        let odd = Address::parse("smb://nas/a%25b c/").unwrap();
        let (_, back) = of_point_name(&point_name("rclone", &odd)).unwrap();
        assert_eq!(back, odd.top());
    }
}
