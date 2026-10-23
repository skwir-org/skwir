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
use crate::rsync::Rsync;

/// Its type in the mount table.
pub const FSTYPE: &str = "fuse.sshfs";

/// How long connecting may take, a password typed included.
const PATIENCE: Duration = Duration::from_secs(300);

/// ssh says a server is gone after this many unanswered keep-alives, 15
/// seconds apart: sshfs then reconnects, and what waited on it fails
/// rather than hangs.
const ALIVE: &[&str] = &["ServerAliveInterval=15", "ServerAliveCountMax=3"];

/// sshfs, as the plugin runs it.
#[derive(Default)]
pub struct Sshfs {
    /// The program ssh asks through: the `skwir` executable running, unless
    /// a test names it.
    pub askpass: Option<std::path::PathBuf>,
    /// ssh options given beside the plugin's (`UserKnownHostsFile=…`): a
    /// test's, so it touches no file of the user's.
    pub ssh_options: Vec<String>,
    /// An ssh configuration file in place of the user's (`-F`): a test's.
    pub ssh_config: Option<std::path::PathBuf>,
    /// rsync through its mounts' connections, made once, with the same
    /// ssh options (ADR-078).
    pub rsync: std::sync::OnceLock<Arc<Rsync>>,
    /// sftp where rsync is not, made once so.
    pub sftp: std::sync::OnceLock<Arc<crate::sftp::Sftp>>,
}

impl Sshfs {
    /// Its rsync (a test reads what it copied).
    pub fn rsync(&self) -> Arc<Rsync> {
        Arc::clone(self.rsync.get_or_init(|| {
            Arc::new(Rsync::with(
                self.ssh_options.clone(),
                self.ssh_config.clone(),
            ))
        }))
    }

    /// Its sftp (a test reads what it copied).
    pub fn sftp(&self) -> Arc<crate::sftp::Sftp> {
        Arc::clone(self.sftp.get_or_init(|| {
            Arc::new(crate::sftp::Sftp::with(
                self.ssh_options.clone(),
                self.ssh_config.clone(),
            ))
        }))
    }
}

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
        #[cfg(target_os = "macos")]
        if !fuse_on_macos() {
            return Some("macFUSE or FUSE-T is not installed".to_owned());
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
        top.ssh_user()?;
        let sshfs = skwir_core::os_path::find_program("sshfs")
            .ok_or_else(|| anyhow::anyhow!("sshfs is not installed"))?;
        let point = mounter::point_for(self.id(), &top)?;
        // Its ssh shares its connection (ADR-078). A socket left by an ssh
        // gone is no one's: this point is not mounted.
        let control = crate::ssh_mount::control_path(&point).filter(|control| {
            control
                .parent()
                .is_some_and(|folder| mounter::make_private(folder).is_ok())
        });
        if let Some(control) = &control {
            let _ = std::fs::remove_file(control);
        }
        let mut command = Command::new(sshfs);
        command.args(arguments(&top, &point, control.as_deref()));
        for option in &self.ssh_options {
            command.arg("-o").arg(option);
        }
        if let Some(config) = &self.ssh_config {
            command.arg("-F").arg(config);
        }
        let attempt = mounter::attempt(cancel);
        let askpass = match &self.askpass {
            Some(program) => mounter::Askpass::serve_with(program.clone(), ask, &attempt)?,
            None => mounter::Askpass::serve(ask, &attempt)?,
        };
        askpass.give(&mut command);
        mounter::mount(&mut command, &point, &attempt, PATIENCE).map_err(|error| {
            let error = anyhow::anyhow!("{}", words(&error.to_string()));
            mounter::ended(&attempt, cancel, error)
        })?;
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
            device: None,
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

    /// rsync first, sftp where rsync does not serve.
    fn transfers(&self) -> Vec<Arc<dyn skwir_core::op::transfer::Transfer>> {
        vec![self.rsync(), self.sftp()]
    }
}

/// What sshfs is given for `top` mounted at `point`: the server's root,
/// so one mount holds every path of it; its ssh the master of `control`.
fn arguments(top: &Address, point: &Path, control: Option<&Path>) -> Vec<std::ffi::OsString> {
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
    let mut options: Vec<String> = vec!["reconnect".into(), "idmap=user".into()];
    options.extend(ALIVE.iter().map(|o| (*o).to_owned()));
    options.extend(
        control
            .map(crate::ssh_mount::master_options)
            .unwrap_or_default(),
    );
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

/// Whether macOS has a FUSE installed for sshfs and rclone to mount
/// through: macFUSE's, or FUSE-T's. Compiled only.
#[cfg(target_os = "macos")]
pub(crate) fn fuse_on_macos() -> bool {
    [
        "/Library/Filesystems/macfuse.fs",
        "/Library/Filesystems/fuse-t.fs",
        "/usr/local/lib/libfuse-t.dylib",
    ]
    .iter()
    .any(|p| std::path::Path::new(p).exists())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sshfs_mounts_a_servers_root_through_the_users_ssh() {
        let top = Address::parse("sftp://me@example.com:2222/home/me")
            .unwrap()
            .top();
        let args = arguments(
            &top,
            Path::new("/run/user/1000/skwir/remote/x"),
            Some(Path::new("/run/user/1000/skwir/ssh/0123")),
        );
        let args: Vec<String> = args
            .iter()
            .map(|a| a.to_string_lossy().into_owned())
            .collect();
        assert_eq!(args[0], "me@example.com:/");
        assert_eq!(args[1], "/run/user/1000/skwir/remote/x");
        assert_eq!(args[2..4], ["-p", "2222"]);
        assert!(args[5].contains("reconnect"));
        assert!(
            args[5].contains("ControlMaster=auto,ControlPath=/run/user/1000/skwir/ssh/0123"),
            "its ssh shares its connection"
        );
        let v6 = Address::parse("sftp://[fe80::1]/").unwrap();
        assert_eq!(
            arguments(&v6, Path::new("/m"), None)[0].to_string_lossy(),
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
