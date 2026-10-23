//! The system's own shares, off Linux (ADR-077, "Windows' shares and
//! macOS's own mounts"): on Windows an SMB share or a WebDAV folder is
//! reached by its UNC path once connected (`WNetAddConnection2W`: no
//! drive letter, for the session alone), its folder that path; on macOS
//! SMB, AFP, NFS and WebDAV are mounted by `NetFS` under /Volumes, as the
//! Finder's Connect to Server does. A password is asked only once the
//! server refused the login without one (Windows' saved credentials, the
//! Keychain, a guest), three times at most, and given in the call alone
//! (`skwir_platform::shares`). Linux mounts with sshfs, GVfs, KIO or
//! rclone: there this connector is not available. Compiled only: nothing
//! here runs on Linux, and Wine connects no share.

use std::path::PathBuf;
use std::sync::Arc;

use skwir_core::cancel::CancelToken;
use skwir_core::fs::reach::Remote;

use crate::address::{Address, Scheme};
use crate::connector::{Ask, Connected, Connector, Recognised};

/// How often a refused login is asked again.
const TRIES: usize = 3;

/// The system's own shares.
pub struct SystemShares;

impl Connector for SystemShares {
    fn id(&self) -> &'static str {
        "system"
    }

    fn title(&self) -> &'static str {
        if cfg!(windows) { "Windows" } else { "macOS" }
    }

    fn schemes(&self) -> &'static [Scheme] {
        if cfg!(windows) {
            &[Scheme::Smb, Scheme::Dav, Scheme::Davs]
        } else {
            &[
                Scheme::Smb,
                Scheme::Afp,
                Scheme::Nfs,
                Scheme::Dav,
                Scheme::Davs,
            ]
        }
    }

    fn unavailable(&self) -> Option<String> {
        (!cfg!(any(windows, target_os = "macos"))).then(|| {
            "the system's own shares are Windows' and macOS's: here sshfs, GVfs, KIO or \
             rclone mount them"
                .to_owned()
        })
    }

    fn connect(
        &self,
        address: &Address,
        ask: &Arc<dyn Ask>,
        cancel: &CancelToken,
    ) -> anyhow::Result<Connected> {
        let top = address.top();
        let mut password: Option<String> = ask.kept_password();
        for _ in 0..=TRIES {
            anyhow::ensure!(!cancel.is_cancelled(), "cancelled");
            match attach(&top, password.as_deref()) {
                Ok(point) => {
                    skwir_core::fs::reach::changed();
                    return Ok(Connected {
                        point,
                        address: top,
                    });
                }
                Err(skwir_platform::shares::Refused::Login) => {
                    let title = format!("Password for {}", top.label());
                    let answer = ask
                        .password(&title, "Connect", cancel)
                        .ok_or_else(|| anyhow::anyhow!("cancelled"))?;
                    password = Some(answer.text);
                }
                Err(skwir_platform::shares::Refused::Other(error)) => return Err(error),
            }
        }
        anyhow::bail!("the server refused the login")
    }

    fn recognises(&self, remote: &Remote) -> Option<Recognised> {
        let address = recognised(remote)?;
        Some(Recognised {
            label: address.label(),
            address: Some(address),
            ours: false,
            device: None,
        })
    }

    fn disconnect(
        &self,
        remote: &Remote,
        recognised: &Recognised,
        cancel: &CancelToken,
    ) -> anyhow::Result<()> {
        if cfg!(windows) {
            let _ = (recognised, cancel);
            skwir_platform::shares::disconnect_unc(&remote.point)?;
            skwir_core::fs::reach::changed();
            Ok(())
        } else {
            crate::mounter::unmount(&remote.point, recognised.ours, cancel)
        }
    }
}

/// Connect `top` as this system does, with `password` where given: where
/// its folder is.
fn attach(
    top: &Address,
    password: Option<&str>,
) -> Result<PathBuf, skwir_platform::shares::Refused> {
    let user = top.user.as_deref();
    if cfg!(windows) {
        let unc = unc_of(top).ok_or_else(|| {
            skwir_platform::shares::Refused::Other(anyhow::anyhow!(
                "“{top}” names no share: smb://server/share"
            ))
        })?;
        let user = match (&top.domain, user) {
            (Some(domain), Some(user)) => Some(format!("{domain}\\{user}")),
            (None, Some(user)) => Some(user.to_owned()),
            _ => None,
        };
        skwir_platform::shares::connect_unc(&unc, user.as_deref(), password)?;
        Ok(unc)
    } else {
        skwir_platform::shares::mount_url(&url_of(top), user, password)
    }
}

/// The UNC path Windows reaches `address`'s top at: `\\server\share` for
/// SMB (its port is not said: Windows asks 445), `\\server@SSL@port\
/// DavWWWRoot` and the folder for WebDAV, through its `WebClient`.
pub fn unc_of(address: &Address) -> Option<PathBuf> {
    let path = String::from_utf8_lossy(&address.path).into_owned();
    let parts: Vec<&str> = path.split('/').filter(|p| !p.is_empty()).collect();
    let host = address.bare_host();
    let mut unc = match address.scheme {
        Scheme::Smb => {
            let share = parts.first()?;
            format!(r"\\{host}\{share}")
        }
        Scheme::Dav | Scheme::Davs => {
            let mut server = host.to_owned();
            if address.scheme == Scheme::Davs {
                server.push_str("@SSL");
            }
            if let Some(port) = address.port {
                server.push('@');
                server.push_str(&port.to_string());
            }
            let mut unc = format!(r"\\{server}\DavWWWRoot");
            for part in &parts {
                unc.push('\\');
                unc.push_str(part);
            }
            unc
        }
        _ => return None,
    };
    if unc.ends_with('\\') {
        unc.pop();
    }
    Some(PathBuf::from(unc))
}

/// The URL `NetFS` mounts `address`'s top by: its scheme's own, WebDAV as
/// `http(s)://`, with its user, never a password.
pub fn url_of(address: &Address) -> String {
    let scheme = match address.scheme {
        Scheme::Dav => "http",
        Scheme::Davs => "https",
        other => other.name(),
    };
    let user = address
        .user
        .as_deref()
        .map(|user| match &address.domain {
            Some(domain) => format!("{domain};{user}@"),
            None => format!("{user}@"),
        })
        .unwrap_or_default();
    let port = address.port.map(|p| format!(":{p}")).unwrap_or_default();
    let path = String::from_utf8_lossy(&address.path).into_owned();
    format!("{scheme}://{user}{}{port}{path}", address.host)
}

/// The address of a share the system mounted: a UNC path's server and
/// share (Windows); a mount whose type is the system's own SMB, AFP, NFS
/// or WebDAV, by its source (macOS: `//me@server/share`, `server:/export`,
/// `https://server/dav`).
fn recognised(remote: &Remote) -> Option<Address> {
    let point = remote.point.to_string_lossy();
    if let Some(rest) = point.strip_prefix(r"\\") {
        let mut parts = rest.split('\\');
        let server = parts.next()?;
        let share = parts.next()?;
        if share.eq_ignore_ascii_case("DavWWWRoot") {
            let mut bits = server.split('@');
            let host = bits.next()?;
            let ssl = server.contains("@SSL");
            let port = bits.find_map(|b| b.parse::<u16>().ok());
            let scheme = if ssl { "davs" } else { "dav" };
            let port = port.map(|p| format!(":{p}")).unwrap_or_default();
            return Address::parse(&format!("{scheme}://{host}{port}/")).ok();
        }
        return Address::parse(&format!("smb://{server}/{share}")).ok();
    }
    let source = remote.source.to_string_lossy();
    let text = match remote.fstype.as_str() {
        "smbfs" => format!("smb:{source}"),
        "afpfs" => format!("afp:{source}"),
        "nfs" => {
            let (host, export) = source.split_once(':')?;
            format!("nfs://{host}{export}")
        }
        "webdav" => source
            .replacen("https://", "davs://", 1)
            .replacen("http://", "dav://", 1),
        _ => return None,
    };
    Address::parse(&text).ok()
}

/// Whether `point` is a folder of a share the system connected (a test of
/// the parsing alone).
#[cfg(test)]
fn is_unc(point: &std::path::Path) -> bool {
    point.to_string_lossy().starts_with(r"\\")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn remote(point: &str, fstype: &str, source: &str) -> Remote {
        Remote {
            point: PathBuf::from(point),
            fstype: fstype.to_owned(),
            source: source.into(),
            device: None,
        }
    }

    /// Windows reaches a share at its UNC path; WebDAV through its
    /// `WebClient`, its port and SSL said in the server's name.
    #[test]
    fn an_address_is_a_unc_path_on_windows() {
        let unc = |text: &str| {
            unc_of(&Address::parse(text).unwrap()).map(|p| p.to_string_lossy().into_owned())
        };
        assert_eq!(
            unc("smb://nas/photos/2024").as_deref(),
            Some(r"\\nas\photos")
        );
        assert_eq!(unc("smb://nas/"), None, "no share named");
        assert_eq!(
            unc("davs://cloud.example.com:8443/remote.php/dav").as_deref(),
            Some(r"\\cloud.example.com@SSL@8443\DavWWWRoot\remote.php\dav")
        );
        assert_eq!(unc("dav://nas/").as_deref(), Some(r"\\nas\DavWWWRoot"));
        assert!(is_unc(std::path::Path::new(r"\\nas\photos")));
    }

    /// `NetFS` mounts a URL of the scheme's own, WebDAV's as HTTP, with its
    /// user and never a password.
    #[test]
    fn an_address_is_a_url_netfs_mounts() {
        let url = |text: &str| url_of(&Address::parse(text).unwrap());
        assert_eq!(url("smb://WORK;me@nas/photos"), "smb://WORK;me@nas/photos");
        assert_eq!(url("davs://me@cloud:8443/dav"), "https://me@cloud:8443/dav");
        assert_eq!(url("nfs://nas/export/media"), "nfs://nas/export/media");
        assert_eq!(url("afp://mac.local/Shared"), "afp://mac.local/Shared");
    }

    /// What the system mounted is known by its address: a UNC path, or
    /// macOS's mount table's source.
    #[test]
    fn a_share_the_system_mounted_is_known_by_its_address() {
        let known = |point, fstype, source| {
            recognised(&remote(point, fstype, source)).map(|a| a.to_string())
        };
        assert_eq!(
            known(r"\\nas\photos", "", "").as_deref(),
            Some("smb://nas/photos")
        );
        assert_eq!(
            known(r"\\cloud@SSL@8443\DavWWWRoot", "", "").as_deref(),
            Some("davs://cloud:8443/")
        );
        assert_eq!(
            known("/Volumes/photos", "smbfs", "//me@nas/photos").as_deref(),
            Some("smb://me@nas/photos")
        );
        assert_eq!(
            known("/Volumes/export", "nfs", "nas:/export").as_deref(),
            Some("nfs://nas/export")
        );
        assert_eq!(
            known("/Volumes/dav", "webdav", "https://cloud/dav").as_deref(),
            Some("davs://cloud/dav")
        );
        assert_eq!(known("/mnt/x", "ext4", "/dev/sda1"), None);
    }
}
