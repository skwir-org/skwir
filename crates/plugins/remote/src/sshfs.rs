//! sshfs: SFTP through the user's own ssh (ADR-077), tried first for
//! `sftp://` (the user's choice, 2026-10-20): ssh's configuration, agent
//! and known hosts hold, and what ssh asks reaches the window through
//! `SSH_ASKPASS`.

use std::path::Path;
use std::process::Command;
use std::sync::Arc;
use std::time::Duration;

use skwir_core::cancel::CancelToken;
use skwir_core::fs::reach::Remote;

use crate::address::{Address, Scheme};
use crate::connector::{Ask, Connected, Connector, Recognised};
use crate::mounter;

/// Its type in the mount table.
pub const FSTYPE: &str = "fuse.sshfs";

/// How long connecting may take, a password typed included.
const PATIENCE: Duration = Duration::from_secs(300);

/// ssh says a server is gone after this many unanswered keep-alives, 15
/// seconds apart: sshfs then reconnects, and what waited on it fails
/// rather than hangs.
const ALIVE: &[&str] = &["ServerAliveInterval=15", "ServerAliveCountMax=3"];

pub struct Sshfs;

impl Connector for Sshfs {
    fn id(&self) -> &'static str {
        "sshfs"
    }

    fn title(&self) -> &'static str {
        "sshfs"
    }

    fn schemes(&self) -> &'static [Scheme] {
        &[Scheme::Sftp]
    }

    fn unavailable(&self) -> Option<String> {
        if cfg!(windows) {
            return Some("sshfs does not run on Windows".to_owned());
        }
        skwir_core::os_path::find_program("sshfs")
            .is_none()
            .then(|| "sshfs is not installed".to_owned())
    }

    fn connect(
        &self,
        address: &Address,
        ask: &Arc<dyn Ask>,
        cancel: &CancelToken,
    ) -> anyhow::Result<Connected> {
        let top = address.top();
        let sshfs = skwir_core::os_path::find_program("sshfs")
            .ok_or_else(|| anyhow::anyhow!("sshfs is not installed"))?;
        let point = mounter::point_for(self.id(), &top)?;
        let mut command = Command::new(sshfs);
        command.args(arguments(&top, &point));
        let askpass = mounter::Askpass::serve(ask, cancel)?;
        askpass.give(&mut command);
        mounter::mount(&mut command, &point, cancel, PATIENCE)
            .map_err(|error| anyhow::anyhow!("{}", words(&error.to_string())))?;
        Ok(Connected {
            point,
            address: top,
        })
    }

    fn recognises(&self, remote: &Remote) -> Option<Recognised> {
        if remote.fstype != FSTYPE {
            return None;
        }
        if let Some(ours) = mounter::ours(self.id(), &remote.point) {
            return Some(ours);
        }
        // Mounted by the user: named as its folder is, its address read
        // from what sshfs was given (`me@host:/srv`; the port is not said).
        let source = remote.source.to_string_lossy();
        let address = of_source(&source);
        Some(Recognised {
            label: remote.point.file_name().map_or_else(
                || source.clone().into_owned(),
                |name| name.to_string_lossy().into_owned(),
            ),
            address,
            ours: false,
        })
    }

    fn disconnect(
        &self,
        remote: &Remote,
        recognised: &Recognised,
        cancel: &CancelToken,
    ) -> anyhow::Result<()> {
        mounter::unmount(&remote.point, recognised.ours, cancel)
    }
}

/// What sshfs is given for `top` mounted at `point`: the server's root,
/// so one mount holds every path of it.
fn arguments(top: &Address, point: &Path) -> Vec<std::ffi::OsString> {
    let mut remote = String::new();
    if let Some(user) = &top.user {
        remote.push_str(user);
        remote.push('@');
    }
    remote.push_str(&top.host);
    remote.push_str(":/");
    let mut args: Vec<std::ffi::OsString> = vec![remote.into(), point.into()];
    if let Some(port) = top.port {
        args.push("-p".into());
        args.push(port.to_string().into());
    }
    let mut options = vec!["reconnect", "idmap=user"];
    options.extend(ALIVE);
    args.push("-o".into());
    args.push(options.join(",").into());
    args
}

/// The address of what sshfs was given, as mountinfo says it:
/// `me@host:/srv`, `host:` (a home).
pub fn of_source(source: &str) -> Option<Address> {
    let (who, path) = source.rsplit_once(':')?;
    if who.is_empty() || who.starts_with('/') {
        return None;
    }
    let path = if path.starts_with('/') {
        path.to_owned()
    } else {
        // Relative to the home, which only the server knows: the top.
        "/".to_owned()
    };
    Address::parse(&format!("sftp://{who}{path}")).ok()
}

/// What ssh and sshfs say on failing, in the user's words where it can be
/// told.
fn words(said: &str) -> String {
    let said = said.trim();
    if said.contains("Permission denied") {
        "the server refused the login (wrong password, or no key it accepts)".to_owned()
    } else if said.contains("Host key verification failed") {
        "the server's identity was not confirmed".to_owned()
    } else if said.contains("REMOTE HOST IDENTIFICATION HAS CHANGED") {
        "the server's identity changed since it was last seen: ssh refuses it \
         (see ~/.ssh/known_hosts)"
            .to_owned()
    } else if said.contains("Could not resolve hostname") {
        "the server's name was not found".to_owned()
    } else if said.contains("Connection refused") {
        "the server refused the connection".to_owned()
    } else if said.contains("Connection timed out") || said.contains("timed out") {
        "the server did not answer".to_owned()
    } else {
        said.lines().last().unwrap_or(said).to_owned()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sshfs_mounts_a_servers_root_through_the_users_ssh() {
        let top = Address::parse("sftp://me@example.com:2222/home/me")
            .unwrap()
            .top();
        let args = arguments(&top, Path::new("/run/user/1000/skwir/remote/x"));
        let args: Vec<String> = args
            .iter()
            .map(|a| a.to_string_lossy().into_owned())
            .collect();
        assert_eq!(args[0], "me@example.com:/");
        assert_eq!(args[1], "/run/user/1000/skwir/remote/x");
        assert_eq!(args[2..4], ["-p", "2222"]);
        assert!(args[5].contains("reconnect"));
        let v6 = Address::parse("sftp://[fe80::1]/").unwrap();
        assert_eq!(
            arguments(&v6, Path::new("/m"))[0].to_string_lossy(),
            "[fe80::1]:/"
        );
    }

    #[test]
    fn a_users_own_sshfs_mount_says_its_server() {
        assert_eq!(
            of_source("me@example.com:/srv/data").unwrap().to_string(),
            "sftp://me@example.com/srv/data"
        );
        assert_eq!(
            of_source("example.com:").unwrap().to_string(),
            "sftp://example.com/"
        );
        assert!(of_source("/dev/sda1").is_none());
    }

    #[test]
    fn what_ssh_says_is_said_plainly() {
        assert!(
            words("me@h: Permission denied (publickey,password).\nread: Connection reset by peer")
                .contains("refused the login")
        );
        assert_eq!(words("something else\nlast line"), "last line");
    }
}
