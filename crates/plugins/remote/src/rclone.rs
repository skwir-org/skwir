//! rclone (ADR-077): the remotes of its configuration (`rclone://gdrive/`,
//! seventy kinds of storage), and SFTP, SMB, WebDAV and FTP addresses
//! (SFTP through the user's own ssh). Mounted so that a file written is on
//! the server once closed (`--vfs-cache-mode minimal`: a file opened to
//! write only goes straight up; one opened to read and write, as an editor
//! does, is kept on the disk and sent after).
//!
//! A server's password (ADR-082): rclone cannot ask, and a WebDAV mount
//! checks nothing until it is read. So an SMB, WebDAV or FTP address is
//! listed once before it is mounted: with the password the keyring keeps,
//! else none, and a refused login asks (`Ask::password`: GVfs's kept one,
//! then the window), three times at most. rclone takes the password
//! obscured (`rclone obscure -`, given it through a pipe) in its
//! environment alone (`RCLONE_SMB_PASS`…), never on a command line.

use std::fmt::Write as _;
use std::path::Path;
use std::process::Command;
use std::sync::Arc;
use std::time::Duration;

use skwir_core::cancel::CancelToken;
use skwir_core::fs::reach::Remote;

use crate::address::{Address, Scheme};
use crate::connector::{Ask, Connected, Connector, Recognised, Server};
use crate::mounter;

/// Its type in the mount table.
pub const FSTYPE: &str = "fuse.rclone";

/// How long connecting may take: a cloud's first listing, a password.
const PATIENCE: Duration = Duration::from_secs(120);

pub struct Rclone;

impl Connector for Rclone {
    fn id(&self) -> &'static str {
        "rclone"
    }

    fn title(&self) -> &'static str {
        "rclone"
    }

    fn schemes(&self) -> &'static [Scheme] {
        &[
            Scheme::Rclone,
            Scheme::Sftp,
            Scheme::Smb,
            Scheme::Dav,
            Scheme::Davs,
            Scheme::Ftp,
            Scheme::Ftps,
        ]
    }

    fn unavailable(&self) -> Option<String> {
        if skwir_core::os_path::find_program("rclone").is_none() {
            return Some("rclone is not installed".to_owned());
        }
        #[cfg(target_os = "linux")]
        if ["fusermount3", "fusermount"]
            .iter()
            .all(|p| skwir_core::os_path::find_program(p).is_none())
        {
            return Some("FUSE (fusermount3) is not installed".to_owned());
        }
        None
    }

    fn connect(
        &self,
        address: &Address,
        ask: &Arc<dyn Ask>,
        cancel: &CancelToken,
    ) -> anyhow::Result<Connected> {
        let top = address.top();
        let rclone = skwir_core::os_path::find_program("rclone")
            .ok_or_else(|| anyhow::anyhow!("rclone is not installed"))?;
        let remote = remote_of(&top)?;
        let password = match password_variable(top.scheme) {
            Some(variable) => logged_in(&rclone, &remote, &top, variable, ask, cancel)?
                .map(|password| (variable, password)),
            None => None,
        };
        let point = mounter::point_for(self.id(), &top)?;
        let mut command = Command::new(rclone);
        command.args(mount_arguments(&remote, &point));
        if let Some((variable, password)) = &password {
            command.env(variable, password);
        }
        if top.scheme == Scheme::Sftp {
            // A word each, split at spaces by rclone: names checked so.
            top.ssh_user()?;
            command.arg("--sftp-ssh").arg(ssh_command(&top));
        }
        // A configuration kept encrypted asks its password of the window.
        let attempt = mounter::attempt(cancel);
        let askpass = mounter::Askpass::serve(ask, &attempt)?;
        askpass.give(&mut command);
        command
            .arg("--password-command")
            .arg(std::env::current_exe()?);
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
        // Mounted by the user: `gdrive:Photos`, or `:sftp{hash}:` for an
        // address, whose parts rclone does not say.
        let source = remote.source.to_string_lossy();
        let address = source
            .split_once(':')
            .filter(|(name, _)| !name.is_empty())
            .and_then(|(name, path)| {
                Address::parse(&format!("rclone://{name}/{}", path.trim_start_matches('/'))).ok()
            });
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

    fn servers(&self, cancel: &CancelToken) -> Vec<Server> {
        let Some(rclone) = skwir_core::os_path::find_program("rclone") else {
            return Vec::new();
        };
        let mut command = Command::new(rclone);
        command.args(["listremotes", "--long"]);
        // An encrypted configuration is not opened to list it.
        command.arg("--ask-password=false");
        match skwir_platform::process::run(&mut command, cancel, Duration::from_secs(10)) {
            Ok(out) => remotes(&String::from_utf8_lossy(&out)),
            Err(error) => {
                tracing::debug!("rclone listremotes: {error:#}");
                Vec::new()
            }
        }
    }
}

/// The remotes `rclone listremotes --long` names: `gdrive: drive`.
fn remotes(listed: &str) -> Vec<Server> {
    listed
        .lines()
        .filter_map(|line| {
            let (name, kind) = line.split_once(':')?;
            let name = name.trim();
            let address = Address::parse(&format!("rclone://{name}/")).ok()?;
            let kind = kind.trim();
            Some(Server {
                label: if kind.is_empty() {
                    name.to_owned()
                } else {
                    format!("{name} ({kind})")
                },
                address,
            })
        })
        .collect()
}

/// What rclone mounts for `top`: a remote of its configuration, or one it
/// is given by its parts (a connection string, `:sftp,host=…:`), a value
/// quoted where it holds what would end it.
fn remote_of(top: &Address) -> anyhow::Result<String> {
    let path = String::from_utf8_lossy(&top.path)
        .trim_start_matches('/')
        .to_owned();
    let quoted = |value: &str| format!("'{}'", value.replace('\'', "''"));
    let mut parts = Vec::new();
    let kind = match top.scheme {
        Scheme::Rclone => return Ok(format!("{}:{path}", top.host)),
        // Through the user's ssh: the host and user go to it, not here.
        Scheme::Sftp => "sftp",
        Scheme::Smb => {
            parts.push(format!("host={}", quoted(top.bare_host())));
            "smb"
        }
        Scheme::Dav | Scheme::Davs => {
            let scheme = if top.scheme == Scheme::Davs {
                "https"
            } else {
                "http"
            };
            let port = top.port.map(|p| format!(":{p}")).unwrap_or_default();
            parts.push(format!(
                "url={}",
                quoted(&format!("{scheme}://{}{port}", top.host))
            ));
            "webdav"
        }
        Scheme::Ftp | Scheme::Ftps => {
            parts.push(format!("host={}", quoted(top.bare_host())));
            if top.scheme == Scheme::Ftps {
                parts.push("tls=true".to_owned());
            }
            "ftp"
        }
        Scheme::Nfs | Scheme::Afp => {
            anyhow::bail!("rclone does not open {} addresses", top.scheme.title())
        }
    };
    if top.scheme != Scheme::Sftp {
        if let Some(user) = &top.user {
            parts.push(format!("user={}", quoted(user)));
        }
        if let (Some(port), false) = (top.port, matches!(top.scheme, Scheme::Dav | Scheme::Davs)) {
            parts.push(format!("port={port}"));
        }
        if let Some(domain) = &top.domain {
            parts.push(format!("domain={}", quoted(domain)));
        }
    }
    let mut remote = format!(":{kind}");
    for part in parts {
        remote.push(',');
        remote.push_str(&part);
    }
    // WebDAV's path is the server's after its address.
    let path = match top.scheme {
        Scheme::Dav | Scheme::Davs => String::from_utf8_lossy(&top.path).into_owned(),
        _ => path,
    };
    Ok(format!("{remote}:{path}"))
}

/// The variable rclone reads the password of an address of `scheme` from,
/// obscured: `None` where it takes none (SFTP's is ssh's; a remote of its
/// configuration keeps its own).
fn password_variable(scheme: Scheme) -> Option<&'static str> {
    match scheme {
        Scheme::Smb => Some("RCLONE_SMB_PASS"),
        Scheme::Dav | Scheme::Davs => Some("RCLONE_WEBDAV_PASS"),
        Scheme::Ftp | Scheme::Ftps => Some("RCLONE_FTP_PASS"),
        Scheme::Sftp | Scheme::Nfs | Scheme::Afp | Scheme::Rclone => None,
    }
}

/// How many times a refused login is asked again.
const TRIES: usize = 3;

/// Log in to `remote` (the address `top`'s), listing its top once: with
/// the password the keyring keeps, else none, then each refusal asking
/// (`Ask::password`), [`TRIES`] times at most. The password that served,
/// obscured for `variable`; `None` where none was needed.
fn logged_in(
    rclone: &Path,
    remote: &str,
    top: &Address,
    variable: &str,
    ask: &Arc<dyn Ask>,
    cancel: &CancelToken,
) -> anyhow::Result<Option<String>> {
    let title = format!("Password for {}", top.label());
    let mut password = ask.kept_password();
    let mut asked = 0;
    loop {
        let given = password
            .as_deref()
            .map(|password| obscured(rclone, password))
            .transpose()?;
        match listed(rclone, remote, variable, given.as_deref(), cancel) {
            Ok(()) => return Ok(given),
            Err(said) if refused(&said) && asked < TRIES => {
                asked += 1;
                let answer = ask
                    .password(&title, "Connect", cancel)
                    .ok_or_else(|| anyhow::anyhow!("you cancelled"))?;
                password = Some(answer.text);
            }
            Err(said) if refused(&said) => {
                anyhow::bail!("the server refused the login (wrong password?)")
            }
            Err(said) => anyhow::bail!("{}", words(&said)),
        }
    }
}

/// Whether rclone reaches `remote`, its top listed, given the obscured
/// `password` in `variable`; else what it said.
fn listed(
    rclone: &Path,
    remote: &str,
    variable: &str,
    password: Option<&str>,
    cancel: &CancelToken,
) -> Result<(), String> {
    let mut command = Command::new(rclone);
    command
        .args([
            "lsf",
            "--max-depth",
            "1",
            "--dirs-only",
            "--ask-password=false",
        ])
        .arg(remote)
        .env_remove(variable);
    if let Some(password) = password {
        command.env(variable, password);
    }
    match skwir_platform::process::run_to_exit(&mut command, None, cancel, PATIENCE) {
        Ok(finished) if finished.status.success() => Ok(()),
        Ok(finished) => Err(finished.stderr),
        Err(error) => Err(format!("{error:#}")),
    }
}

/// `password` obscured as rclone reads it (`rclone obscure -`), given it
/// through a pipe: never a command line, never a file.
fn obscured(rclone: &Path, password: &str) -> anyhow::Result<String> {
    use std::io::Write as _;
    use std::process::Stdio;
    let mut child = Command::new(rclone)
        .args(["obscure", "-"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()?;
    // A password fits a pipe's buffer: written whole before it is read.
    if let Some(mut input) = child.stdin.take() {
        input.write_all(password.as_bytes())?;
        input.write_all(b"\n")?;
    }
    let out = child.wait_with_output()?;
    anyhow::ensure!(out.status.success(), "rclone did not obscure the password");
    let text = String::from_utf8(out.stdout)?;
    Ok(text.trim().to_owned())
}

/// Whether what rclone said is the server refusing the login, or its
/// wanting a password it was not given.
fn refused(said: &str) -> bool {
    [
        "401 Unauthorized",
        "530 ",
        "not logged in",
        "Login incorrect",
        "LOGON_FAILURE",
        "logon failure",
        "is it obscured",
        "attempted logon is invalid",
    ]
    .iter()
    .any(|sign| said.contains(sign))
}

/// The ssh rclone runs for an SFTP address: the user's own, its
/// configuration and agent, a server gone told within a minute.
fn ssh_command(top: &Address) -> String {
    let mut ssh = String::from("ssh -o ServerAliveInterval=15 -o ServerAliveCountMax=3");
    if let Some(port) = top.port {
        let _ = write!(ssh, " -p {port}");
    }
    ssh.push(' ');
    if let Some(user) = &top.user {
        ssh.push_str(user);
        ssh.push('@');
    }
    ssh.push_str(top.bare_host());
    ssh
}

/// `rclone mount`'s arguments for `remote` at `point`.
fn mount_arguments(remote: &str, point: &Path) -> Vec<std::ffi::OsString> {
    let mut args: Vec<std::ffi::OsString> = vec!["mount".into(), remote.into(), point.into()];
    args.extend(
        [
            "--daemon",
            "--daemon-wait",
            "60s",
            "--vfs-cache-mode",
            "minimal",
            // What the server changed is seen within a minute where it
            // says (and on F5 and focus everywhere).
            "--dir-cache-time",
            "1m",
            "--log-level",
            "ERROR",
        ]
        .map(std::ffi::OsString::from),
    );
    args
}

/// What rclone says on failing, in the user's words where it can be told.
fn words(said: &str) -> String {
    let said = said.trim();
    let line = said
        .lines()
        .rev()
        .find(|l| l.contains("CRITICAL") || l.contains("ERROR"))
        .unwrap_or_else(|| said.lines().last().unwrap_or(said));
    // `2026/10/21 00:00:00 CRITICAL: Failed to create file system for …: …`
    let line = line.split_once(": ").map_or(line, |(head, rest)| {
        if head.contains("CRITICAL") || head.contains("ERROR") {
            rest
        } else {
            line
        }
    });
    if line.contains("didn't find section in config file") {
        "rclone's configuration names no such remote".to_owned()
    } else if line.contains("connection refused") {
        "the server refused the connection".to_owned()
    } else if line.contains("no such host") {
        "the server's name was not found".to_owned()
    } else {
        line.to_owned()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rclone_is_given_an_address_by_its_parts() {
        let top = |text: &str| Address::parse(text).unwrap().top();
        assert_eq!(
            remote_of(&top("rclone://gdrive/Photos")).unwrap(),
            "gdrive:"
        );
        assert_eq!(
            remote_of(&top("smb://WORK;me@nas/photos/2024")).unwrap(),
            ":smb,host='nas',user='me',domain='WORK':photos/"
        );
        assert_eq!(
            remote_of(&top("davs://cloud.example.com:8443/remote.php/webdav")).unwrap(),
            ":webdav,url='https://cloud.example.com:8443':/"
        );
        assert_eq!(remote_of(&top("sftp://me@h:2222/x")).unwrap(), ":sftp:");
        assert_eq!(
            ssh_command(&top("sftp://me@h:2222/x")),
            "ssh -o ServerAliveInterval=15 -o ServerAliveCountMax=3 -p 2222 me@h"
        );
        assert!(remote_of(&top("nfs://h/export")).is_err());
        assert_eq!(
            remote_of(&top("ftp://o'brien@h/")).unwrap(),
            ":ftp,host='h',user='o''brien':",
            "a quote doubled"
        );
    }

    #[test]
    fn a_refused_login_is_told_from_another_failure() {
        assert!(refused(
            "couldn't list files: 401 Unauthorized: 401 Unauthorized"
        ));
        assert!(refused(
            "failed to make FTP connection to \"h:21\": 530 \"Incorrect password, not logged in\""
        ));
        assert!(refused(
            "NewFS decrypt password: input too short when revealing password - is it obscured?"
        ));
        assert!(refused(
            "couldn't connect SMB: response error: The attempted logon is invalid. This is either due to a bad username or authentication information."
        ));
        assert!(!refused(
            "dial tcp 127.0.0.1:1: connect: connection refused"
        ));
        assert_eq!(password_variable(Scheme::Davs), Some("RCLONE_WEBDAV_PASS"));
        assert_eq!(password_variable(Scheme::Sftp), None, "ssh's");
    }

    #[test]
    fn the_configurations_remotes_are_servers() {
        let servers = remotes("gdrive:     drive\nnas:        sftp\n");
        assert_eq!(servers.len(), 2);
        assert_eq!(servers[0].address.to_string(), "rclone://gdrive/");
        assert_eq!(servers[0].label, "gdrive (drive)");
    }

    #[test]
    fn rclone_mounts_so_a_copy_is_on_the_server_once_closed() {
        let args = mount_arguments("gdrive:", Path::new("/m"));
        let args: Vec<_> = args
            .iter()
            .map(|a| a.to_string_lossy().into_owned())
            .collect();
        let at = args.iter().position(|a| a == "--vfs-cache-mode").unwrap();
        assert_eq!(args[at + 1], "minimal");
        assert_eq!(args[..3], ["mount", "gdrive:", "/m"]);
    }
}
