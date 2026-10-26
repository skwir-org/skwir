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
    /// A server's password (ADR-082): the keyring's first where the
    /// connection keeps one (`passwords::Keyed`), else asked as a secret
    /// the window offers to remember. Never a key's passphrase, a code, a
    /// configuration's password: those are `secret`.
    fn password(&self, title: &str, accept: &str, cancel: &CancelToken) -> Option<Password> {
        self.secret(title, accept, cancel).map(|text| Password {
            text,
            remember: false,
        })
    }
    /// A server's password the keyring keeps, given once a connection, asked
    /// of no one: for a tool that cannot ask (rclone), given before it
    /// tries.
    fn kept_password(&self) -> Option<String> {
        None
    }
}

/// A server's password as it was answered: whether the user asked for it
/// to be kept (the window's Remember). No `Debug`: it holds the secret.
pub struct Password {
    pub text: String,
    pub remember: bool,
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

/// A device plugged in that a connector's program offers to mount (a phone,
/// a camera), mounted or not (ADR-096): listed among the sidebar's devices,
/// mounted once opened.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Offered {
    /// Where it opens: the top of its mount, once mounted.
    pub address: Address,
    /// What its program calls it ("Example Phone"): mounted or not.
    pub name: String,
    pub device: Device,
    /// Its node on this machine (`/dev/bus/usb/001/004`), which a program
    /// using the device holds open: who holds it is found there.
    pub node: Option<PathBuf>,
}

/// A server the user can connect to, from the tool's configuration (an
/// rclone remote, a network entry of fstab, one of Dolphin's places) or
/// one added by its address (`servers`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Server {
    pub address: Address,
    pub label: String,
    /// What kind of storage its tool says it is (rclone's type: `drive`,
    /// `s3`), empty where it says none.
    pub kind: String,
    /// Who keeps it, and changes it.
    pub kept: crate::servers::Kept,
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
    /// [`Connector::connect`] with the options the user chose for the
    /// server (ADR-086): those its tool takes, the rest left. A tool that
    /// takes none (GVfs, KIO) connects as ever.
    ///
    /// # Errors
    /// As [`Connector::connect`].
    fn connect_with(
        &self,
        address: &Address,
        _options: &crate::options::MountOptions,
        ask: &Arc<dyn Ask>,
        cancel: &CancelToken,
    ) -> anyhow::Result<Connected> {
        self.connect(address, ask, cancel)
    }
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
    /// The program holding a device it opens through (`kiod6`, which holds
    /// the phones KDE's kmtpd keeps, ADR-097), as that program was started:
    /// a device that program holds is said to be openable by this tool.
    fn reaches_through(&self) -> Option<&'static str> {
        None
    }
    /// Whether it opens `address` here, where it may answer for some of a
    /// scheme's addresses alone (KDE's kmtpd, the phones it holds, ADR-097):
    /// one it does not open goes to the next tool, unsaid. Asked before
    /// connecting, on its thread: it may ask a program.
    fn opens(&self, _address: &Address) -> bool {
        true
    }
    /// What on the session bus says the devices plugged in that its
    /// program would mount (a phone, a camera: ADR-096, ADR-097) may have
    /// changed, as match rules (`type='signal',interface='…'`): the plugin
    /// follows them all on one connection, the program's (`bus_env`), and
    /// asks `devices` again when one of a tool's is said. None where it
    /// offers none.
    fn device_signals(&self) -> Vec<String> {
        Vec::new()
    }
    /// The devices plugged in that its program would mount, now. Blocks,
    /// asking programs (on connections of its own, each question given up
    /// on after a while): on a thread of its own.
    fn devices(&self) -> Vec<Offered> {
        Vec::new()
    }
    /// The variables naming the bus its devices are followed on: a test's
    /// own; none for the session's, which the program shares
    /// (`skwir_platform::bus`).
    fn bus_env(&self) -> Vec<(std::ffi::OsString, std::ffi::OsString)> {
        Vec::new()
    }
    /// The servers its configuration names. Reads files and may start the
    /// tool: on a worker.
    fn servers(&self, _cancel: &CancelToken) -> Vec<Server> {
        Vec::new()
    }
    /// What copies many files at once between this machine and a mount it
    /// made (ADR-078), where its tool has a way: sshfs's, rsync through
    /// its connection.
    fn transfers(&self) -> Vec<std::sync::Arc<dyn skwir_core::op::transfer::Transfer>> {
        Vec::new()
    }
    /// Whether a mount of its holds only the folders asked of it so far
    /// (kio-fuse mounts a path, a server's other folders once asked): an
    /// address on a server it mounted is connected again, which costs its
    /// tool a look at that path.
    fn mounts_each_path(&self) -> bool {
        false
    }
    /// What its program is told before a folder of `remote` is read again,
    /// where it answers from what it read before (`Known::forget`).
    fn forget(&self, _remote: &Remote) -> Option<skwir_core::fs::reach::Forget> {
        None
    }
    /// Whether its program reads a file of `remote` whole before any part
    /// of it is read (kio-fuse over WebDAV, FTP, NFS): a thumbnail of a
    /// video would download it all, so Automatic shows small files' alone
    /// there, even near.
    fn reads_whole(&self, _remote: &Remote) -> bool {
        false
    }
}

/// Where Skwir mounts what it connects itself (sshfs, rclone): a folder
/// of its own under the user's runtime folder, one per address, named so
/// that the address can be read back from it after Skwir restarts
/// (principle 3: nothing kept but the mount itself).
pub fn mounts_folder() -> Option<PathBuf> {
    // macOS has no runtime folder: the user's own temporary one, which no
    // other user enters. Never the cache, which a start sweeps
    // (`skwir_core::cache`).
    #[cfg(target_os = "macos")]
    {
        Some(std::env::temp_dir().join("skwir").join("remote"))
    }
    // Windows: Skwir's local application data, the user's own (ADR-086:
    // rclone's mounts through WinFsp are folders there).
    #[cfg(not(unix))]
    {
        skwir_core::folders::state().map(|dir| dir.join("remote"))
    }
    #[cfg(all(unix, not(target_os = "macos")))]
    {
        skwir_core::xdg::runtime_dir().map(|dir| dir.join("skwir").join("remote"))
    }
}

/// The mount point's name for `address` mounted by `connector`:
/// `sshfs sftp:me@example.com:2222`, readable back by [`of_point_name`].
pub fn point_name(connector: &str, address: &Address) -> String {
    let text = address.top().to_string();
    // A folder's name holds no `/`: written as GVfs writes its own, `%2F`.
    let mut text = text.replace('%', "%25").replace('/', "%2F");
    // Nor, on Windows, what its names cannot hold.
    if cfg!(windows) {
        for (c, written) in [
            (':', "%3A"),
            ('\\', "%5C"),
            ('*', "%2A"),
            ('?', "%3F"),
            ('"', "%22"),
            ('<', "%3C"),
            ('>', "%3E"),
            ('|', "%7C"),
        ] {
            text = text.replace(c, written);
        }
    }
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
