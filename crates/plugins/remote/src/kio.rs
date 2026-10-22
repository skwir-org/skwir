//! KIO, KDE's own remotes (ADR-081): kio-fuse, KDE's FUSE bridge to KIO,
//! mounts an address it is asked over the session bus (`mountUrl`), each
//! server a folder of its one mount (`smb/nas`, `ftp/me@host:2121`), shared
//! with every program of the session. What a server asks (a password) is
//! asked in KDE's own dialog and kept in `KWallet` if the user says so:
//! Skwir never sees it. Linux only: kio-fuse runs nowhere else.
//!
//! What kio-fuse does not do, read in its source (5.1.1) and tried against
//! servers on this machine (2026-10-22):
//! - It asks nothing: its KIO jobs have no window, so KIO tells its
//!   workers not to ask (`no-auth-prompt`), and a server that wants a
//!   password fails, as "No such file or directory", unless KDE holds it
//!   already (`KWallet`, or this session's cache). So where kio-fuse fails,
//!   `kioclient --interactive ls` lets KDE ask, its own dialog keeping the
//!   answer for the session (and in `KWallet` if the user says so), and
//!   kio-fuse is asked again. Where it still fails (no such folder, the
//!   dialog cancelled), `kioclient` ends without a word or a window, and
//!   kio-fuse's reason is told (KIO's own word for a WebDAV login refused
//!   is "does not exist").
//! - On a certificate not trusted it waits for good (an answer no one is
//!   asked; three minutes tried): over HTTPS, KDE is let ask first, its
//!   answer kept for the session, unless kio-fuse holds the server. KIO
//!   ends the job a certificate was accepted in at the server's password
//!   (as "does not exist"), so a failure there is asked again once: the
//!   password then, or the certificate again where it was refused.
//! - Once a server is mounted it gives any path on it without looking: a
//!   folder the server lacks fails as it is listed.
//! - It refuses `mtp:` and lets go of no server: its folders stay until it
//!   ends with the session.
//! - It keeps a listing and a file's attributes 30 seconds: before a folder
//!   is read again it is told to forget them (`forceNodeTimeout`).
//! - Over WebDAV, FTP and NFS it reads a file whole into a file of its own
//!   before any part of it is read, and sends one up whole as it is closed,
//!   where the server's refusal is told (`op::sys::close`). SMB, SFTP and
//!   AFC are read and written in place, a request per part.

use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::sync::Arc;
#[cfg(target_os = "linux")]
use std::time::Duration;

use skwir_core::cancel::CancelToken;
use skwir_core::fs::reach::{Forget, Remote};

use crate::address::{Address, Scheme};
use crate::connector::{Ask, Connected, Connector, Device, Recognised, Server};

/// kio-fuse's type in the mount table.
pub const BRIDGE: &str = "fuse.kio-fuse";

/// kio-fuse on the session bus.
#[cfg(target_os = "linux")]
const SERVICE: &str = "org.kde.KIOFuse";
#[cfg(target_os = "linux")]
const OBJECT: &str = "/org/kde/KIOFuse";

/// How long connecting may take: the password KDE's dialog asks for
/// meanwhile is the user's to type. Cancelling stops waiting sooner.
#[cfg(target_os = "linux")]
const PATIENCE: Duration = Duration::from_secs(600);

/// KIO, as the plugin asks it.
#[derive(Default)]
pub struct Kio {
    /// Variables kio-fuse is reached with beside skwir's own: a test's
    /// private session bus, never the user's.
    pub env: Vec<(OsString, OsString)>,
    /// The runtime folder kio-fuse mounts in, when not the user's (a
    /// test's).
    pub runtime: Option<PathBuf>,
    /// Its copies in batches (ADR-078).
    pub kio_copy: std::sync::OnceLock<Arc<crate::kio_copy::KioCopy>>,
}

impl Kio {
    /// What copies in batches to and from its servers (`kioclient`).
    pub fn kio_copy(&self) -> Arc<crate::kio_copy::KioCopy> {
        Arc::clone(self.kio_copy.get_or_init(|| {
            Arc::new(crate::kio_copy::KioCopy::new(
                self.env.clone(),
                self.runtime.clone(),
            ))
        }))
    }
}

impl Kio {
    /// Whether kio-fuse runs on the session's bus (the test's), or would
    /// be started there when asked.
    fn can_start(&self) -> bool {
        #[cfg(target_os = "linux")]
        {
            crate::bus::can_start(SERVICE, &self.env)
        }
        #[cfg(not(target_os = "linux"))]
        {
            let _ = &self.env;
            false
        }
    }

    /// The runtime folder whose `kio-fuse-…` folder is this session's
    /// kio-fuse.
    fn runtime(&self) -> Option<PathBuf> {
        self.runtime.clone().or_else(skwir_core::xdg::runtime_dir)
    }

    /// Ask kio-fuse to mount `url`, and say where it is. Waits for the
    /// password KDE's dialog may ask for, until `cancel` fires.
    #[cfg(target_os = "linux")]
    fn mount_url(&self, url: &str, cancel: &CancelToken) -> anyhow::Result<PathBuf> {
        let (tx, rx) = crossbeam_channel::bounded(1);
        let (env, asked) = (self.env.clone(), url.to_owned());
        std::thread::Builder::new()
            .name("skwir-kio".into())
            .spawn(move || {
                let _ = tx.send(crate::bus::call::<_, String>(
                    &env,
                    SERVICE,
                    OBJECT,
                    "org.kde.KIOFuse.VFS",
                    "mountUrl",
                    &(asked,),
                    PATIENCE,
                ));
            })?;
        loop {
            match rx.recv_timeout(Duration::from_millis(100)) {
                Ok(answer) => return answer.map(PathBuf::from).map_err(|e| said(&e)),
                Err(crossbeam_channel::RecvTimeoutError::Timeout) => {
                    anyhow::ensure!(!cancel.is_cancelled(), "cancelled");
                }
                Err(crossbeam_channel::RecvTimeoutError::Disconnected) => {
                    anyhow::bail!("kio-fuse did not answer");
                }
            }
        }
    }
}

impl Kio {
    /// Let KDE ask what kio-fuse cannot (a password, a certificate):
    /// `kioclient --interactive ls`, its job let ask, KDE's own dialog (in
    /// `kiod`) on the desktop, in the user's language, keeping the answer
    /// for the session. Whether it could then list `url`. `ls`, not `stat`:
    /// its job shows no progress window, so `kioclient` ends at once where
    /// it fails, showing no error box of its own (one would wait to be
    /// dismissed, then Skwir tell the failure again): kio-fuse's reason is
    /// the one told. `None` where no `kioclient` is installed. Waits for
    /// the user: on a worker.
    #[cfg(target_os = "linux")]
    fn asked_by_kde(&self, url: &str, cancel: &CancelToken) -> Option<bool> {
        let kioclient = ["kioclient", "kioclient6"]
            .into_iter()
            .find_map(skwir_core::os_path::find_program)?;
        let mut command = std::process::Command::new(kioclient);
        command
            .envs(self.env.iter().map(|(k, v)| (k, v)))
            .args(["--interactive", "ls", url])
            .stdout(std::process::Stdio::null());
        Some(
            skwir_platform::process::run_to_exit(&mut command, None, cancel, PATIENCE)
                .is_ok_and(|finished| finished.status.success()),
        )
    }

    /// Whether this session's kio-fuse holds `address`'s server already (a
    /// folder of its mount): what was to be asked there was.
    #[cfg(target_os = "linux")]
    fn holds(&self, address: &Address) -> bool {
        let server = server(address);
        skwir_core::fs::reach::all()
            .iter()
            .filter(|remote| remote.fstype == BRIDGE)
            .flat_map(skwir_core::fs::reach::bridged)
            .any(|remote| {
                self.recognises(&remote)
                    .is_some_and(|known| known.address.as_ref() == Some(&server))
            })
    }
}

/// What kio-fuse said on failing to mount, without the address it repeats
/// in its own escaping (`KIOFuse failed to mount smb://nas/x: Permission
/// denied`): the system's words for its error, which hold no colon.
#[cfg(target_os = "linux")]
fn said(error: &anyhow::Error) -> anyhow::Error {
    let text = error.to_string();
    let reason = text
        .rsplit_once(": ")
        .map_or(text.as_str(), |(_, reason)| reason);
    anyhow::anyhow!("KIO could not open it: {reason}")
}

/// KIO's name of `scheme`, where it opens it.
fn kio_scheme(scheme: Scheme) -> Option<&'static str> {
    match scheme {
        Scheme::Smb => Some("smb"),
        Scheme::Dav => Some("webdav"),
        Scheme::Davs => Some("webdavs"),
        Scheme::Ftp => Some("ftp"),
        Scheme::Sftp => Some("sftp"),
        Scheme::Nfs => Some("nfs"),
        Scheme::Ftps | Scheme::Afp | Scheme::Rclone => None,
    }
}

/// The scheme a folder of kio-fuse's mount names (`smb`, `webdav`), and
/// whether KIO reaches a server by it or a device (`afc`, an iPhone);
/// `None` for what is no remote (`tar`, `zip`, `file`).
fn of_folder(name: &str) -> Option<Result<Scheme, Device>> {
    Some(Ok(match name {
        "smb" | "cifs" => Scheme::Smb,
        "webdav" | "dav" => Scheme::Dav,
        "webdavs" | "davs" => Scheme::Davs,
        "ftp" => Scheme::Ftp,
        "sftp" => Scheme::Sftp,
        "nfs" => Scheme::Nfs,
        "afc" => return Some(Err(Device::Phone)),
        _ => return None,
    }))
}

/// Whether kio-fuse reads a file of a server reached by the folder `name`
/// whole before any part (module documentation).
fn read_whole(name: &str) -> bool {
    matches!(name, "webdav" | "dav" | "webdavs" | "davs" | "ftp" | "nfs")
}

/// `address` as KIO opens it: `webdav://host/path` for `dav://host/path`.
fn kio_url(address: &Address) -> Option<String> {
    let uri = address.uri();
    let (_, rest) = uri.split_once("://")?;
    Some(format!("{}://{rest}", kio_scheme(address.scheme)?))
}

/// The server `address` is on, at its top, as kio-fuse keeps it: one
/// folder each (its SMB shares below).
fn server(address: &Address) -> Address {
    let mut server = address.clone();
    server.path = b"/".to_vec();
    server
}

impl Kio {
    /// The folders below this session's kio-fuse mount to `point` (`smb`,
    /// `nas`), when it is in it.
    fn below_mount<'a>(&self, point: &'a Path) -> Option<Vec<&'a str>> {
        let runtime = self.runtime()?;
        let rest = point.strip_prefix(&runtime).ok()?;
        let mut parts = rest.components().map(|c| c.as_os_str().to_str());
        if !parts.next()??.starts_with("kio-fuse-") {
            return None;
        }
        parts.collect()
    }
}

impl Connector for Kio {
    fn id(&self) -> &'static str {
        "kio"
    }

    fn title(&self) -> &'static str {
        "KIO"
    }

    fn schemes(&self) -> &'static [Scheme] {
        // Not SFTP, sshfs's (its questions in the window, and faster), nor
        // NFS, the kernel's: their folders kio-fuse holds are known all the
        // same (`recognises`).
        &[Scheme::Smb, Scheme::Dav, Scheme::Davs, Scheme::Ftp]
    }

    fn unavailable(&self) -> Option<String> {
        if !cfg!(target_os = "linux") {
            return Some("KIO's folders (kio-fuse) are on Linux only".to_owned());
        }
        if !self.can_start() {
            return Some("kio-fuse (KDE) is not installed".to_owned());
        }
        None
    }

    fn connect(
        &self,
        address: &Address,
        _ask: &Arc<dyn Ask>,
        cancel: &CancelToken,
    ) -> anyhow::Result<Connected> {
        #[cfg(target_os = "linux")]
        {
            let url = kio_url(address).ok_or_else(|| {
                anyhow::anyhow!("KIO opens no {} address", address.scheme.title())
            })?;
            // kio-fuse asks nothing: KDE asks (module documentation), then
            // kio-fuse finds what it kept. Over TLS, KDE first: on a
            // certificate not trusted kio-fuse would wait for good. KIO
            // ends the job a certificate was accepted in at the server's
            // password ("does not exist"): a second job asks it.
            let local = if address.scheme == Scheme::Davs && !self.holds(address) {
                let asked = match self.asked_by_kde(&url, cancel) {
                    Some(false) if !cancel.is_cancelled() => self.asked_by_kde(&url, cancel),
                    asked => asked,
                };
                anyhow::ensure!(!cancel.is_cancelled(), "cancelled");
                anyhow::ensure!(asked != Some(false), "KIO could not open it");
                self.mount_url(&url, cancel)?
            } else {
                match self.mount_url(&url, cancel) {
                    Ok(local) => local,
                    Err(first) => {
                        let asked = self.asked_by_kde(&url, cancel);
                        anyhow::ensure!(!cancel.is_cancelled(), "cancelled");
                        if asked != Some(true) {
                            return Err(first);
                        }
                        self.mount_url(&url, cancel)?
                    }
                }
            };
            skwir_core::fs::reach::changed();
            let remote = skwir_core::fs::reach::of(&local)
                .filter(|remote| remote.fstype == BRIDGE)
                .ok_or_else(|| {
                    anyhow::anyhow!("kio-fuse mounted it but its folder is not there")
                })?;
            Ok(Connected {
                point: remote.point.clone(),
                address: server(address),
            })
        }
        #[cfg(not(target_os = "linux"))]
        {
            let _ = (kio_url(address), server(address), cancel);
            anyhow::bail!("KIO's folders (kio-fuse) are on Linux only")
        }
    }

    fn recognises(&self, remote: &Remote) -> Option<Recognised> {
        if remote.fstype != BRIDGE {
            return None;
        }
        // A server's folder of this session's kio-fuse, two below its
        // mount: `smb/nas`.
        let below = self.below_mount(&remote.point)?;
        let [kind, authority] = below.as_slice() else {
            return None;
        };
        Some(match of_folder(kind)? {
            Ok(scheme) => {
                let address = Address::parse(&format!("{}://{authority}/", scheme.name())).ok()?;
                Recognised {
                    label: address.label(),
                    address: Some(address),
                    ours: false,
                    device: None,
                }
            }
            Err(device) => Recognised {
                address: None,
                label: (*authority).to_owned(),
                ours: false,
                device: Some(device),
            },
        })
    }

    fn disconnect(
        &self,
        _remote: &Remote,
        _recognised: &Recognised,
        _cancel: &CancelToken,
    ) -> anyhow::Result<()> {
        anyhow::bail!("KIO keeps its folders until the session ends")
    }

    fn mounts_each_path(&self) -> bool {
        true
    }

    fn transfers(&self) -> Vec<Arc<dyn skwir_core::op::transfer::Transfer>> {
        vec![self.kio_copy()]
    }

    fn forget(&self, remote: &Remote) -> Option<Forget> {
        self.recognises(remote)?;
        #[cfg(target_os = "linux")]
        {
            let env = self.env.clone();
            Some(Forget(Arc::new(move || {
                if let Err(error) = crate::bus::call::<_, ()>(
                    &env,
                    SERVICE,
                    OBJECT,
                    "org.kde.KIOFuse.Private",
                    "forceNodeTimeout",
                    &(),
                    Duration::from_secs(5),
                ) {
                    tracing::warn!("kio-fuse was not told to read its servers again: {error:#}");
                }
            })))
        }
        #[cfg(not(target_os = "linux"))]
        {
            None
        }
    }

    fn servers(&self, _cancel: &CancelToken) -> Vec<Server> {
        // KDE's places (Dolphin's sidebar), the remote ones.
        skwir_core::xdg::data_home()
            .and_then(|data| std::fs::read_to_string(data.join("user-places.xbel")).ok())
            .map(|xml| kde_places(&xml))
            .unwrap_or_default()
    }

    fn reads_whole(&self, remote: &Remote) -> bool {
        self.below_mount(&remote.point)
            .and_then(|below| below.first().copied())
            .is_some_and(read_whole)
    }
}

/// The remote places of KDE's `user-places.xbel` (Dolphin's sidebar):
/// those of a protocol Skwir opens, not hidden, named by their title.
fn kde_places(xml: &str) -> Vec<Server> {
    // Nested no deeper than the parser's recursion can go.
    if skwir_platform::inspection::xml_depth(xml.as_bytes()).is_err() {
        return Vec::new();
    }
    let Ok(doc) = roxmltree::Document::parse(xml) else {
        return Vec::new();
    };
    doc.descendants()
        .filter(|node| node.has_tag_name("bookmark"))
        .filter(|node| {
            !node
                .descendants()
                .any(|n| n.has_tag_name("IsHidden") && n.text() == Some("true"))
        })
        .filter_map(|node| {
            let address = Address::parse(node.attribute("href")?).ok()?;
            let label = node
                .children()
                .find(|n| n.has_tag_name("title"))
                .and_then(|n| n.text())
                .map_or_else(|| address.label(), str::to_owned);
            Some(Server { address, label })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn kde_s_remote_places_are_servers() {
        let xml = r#"<?xml version="1.0" encoding="UTF-8"?>
<xbel xmlns:bookmark="http://www.freedesktop.org/standards/desktop-bookmarks">
 <bookmark href="file:///home/me"><title>Home</title></bookmark>
 <bookmark href="smb://nas/photos"><title>Photos</title>
  <info><metadata owner="http://www.kde.org"><IsHidden>false</IsHidden></metadata></info>
 </bookmark>
 <bookmark href="webdavs://cloud.example.com/remote.php/dav"><title>Cloud</title></bookmark>
 <bookmark href="sftp://me@box/srv"><title>Box</title>
  <info><metadata owner="http://www.kde.org"><IsHidden>true</IsHidden></metadata></info>
 </bookmark>
 <bookmark href="trash:/"><title>Trash</title></bookmark>
 <bookmark href="ftp://mirror.example.org/pub"/>
</xbel>"#;
        let places: Vec<(String, String)> = kde_places(xml)
            .into_iter()
            .map(|s| (s.label, s.address.to_string()))
            .collect();
        assert_eq!(
            places,
            [
                ("Photos".to_owned(), "smb://nas/photos".to_owned()),
                (
                    "Cloud".to_owned(),
                    "davs://cloud.example.com/remote.php/dav".to_owned()
                ),
                (
                    "mirror.example.org".to_owned(),
                    "ftp://mirror.example.org/pub".to_owned()
                ),
            ],
            "not a local folder, the trash, nor a hidden place"
        );
    }

    fn kio() -> Kio {
        Kio {
            runtime: Some(PathBuf::from("/run/user/1000")),
            ..Kio::default()
        }
    }

    fn remote(point: &str) -> Remote {
        Remote {
            point: PathBuf::from(point),
            fstype: BRIDGE.to_owned(),
            source: "kio-fuse".into(),
            device: None,
        }
    }

    #[test]
    fn an_address_is_opened_by_kios_own_scheme() {
        let dav = Address::parse("dav://me@nas.local:8080/Two Words/").unwrap();
        assert_eq!(
            kio_url(&dav).as_deref(),
            Some("webdav://me@nas.local:8080/Two%20Words/")
        );
        let smb = Address::parse("smb://WORK;me@nas/photos").unwrap();
        assert_eq!(kio_url(&smb).as_deref(), Some("smb://WORK;me@nas/photos"));
        assert_eq!(server(&smb).to_string(), "smb://WORK;me@nas/");
        let afp = Address::parse("afp://nas/x").unwrap();
        assert_eq!(kio_url(&afp), None, "KIO opens no AFP");
    }

    #[test]
    fn each_server_of_this_sessions_kio_fuse_is_known_by_its_folder() {
        let kio = kio();
        let share = kio
            .recognises(&remote("/run/user/1000/kio-fuse-AbCdEf/smb/WORK;me@nas"))
            .unwrap();
        assert_eq!(
            share.address,
            Some(Address::parse("smb://WORK;me@nas/").unwrap())
        );
        let dav = kio
            .recognises(&remote("/run/user/1000/kio-fuse-AbCdEf/webdavs/nas:8443"))
            .unwrap();
        assert_eq!(
            dav.address.as_ref().map(ToString::to_string).as_deref(),
            Some("davs://nas:8443/")
        );
        assert!(
            Address::parse("davs://nas:8443/photos")
                .ok()
                .and_then(|a| dav.address.as_ref()?.below(&a).map(<[u8]>::to_vec))
                .is_some_and(|below| below == b"photos"),
            "what is below the server is below its folder"
        );
        let phone = kio
            .recognises(&remote("/run/user/1000/kio-fuse-AbCdEf/afc/0123abcd"))
            .unwrap();
        assert_eq!(phone.device, Some(Device::Phone));
        assert!(
            kio.recognises(&remote("/run/user/1000/kio-fuse-AbCdEf/tar/x.tar"))
                .is_none(),
            "an archive KIO reads is no remote"
        );
        assert!(
            kio.recognises(&remote("/run/user/1001/kio-fuse-AbCdEf/smb/nas"))
                .is_none(),
            "another session's"
        );
        assert!(kio.reads_whole(&remote("/run/user/1000/kio-fuse-AbCdEf/webdav/nas")));
        assert!(!kio.reads_whole(&remote("/run/user/1000/kio-fuse-AbCdEf/smb/nas")));
    }
}
