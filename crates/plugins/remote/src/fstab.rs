//! The system's own network mounts (ADR-077): an NFS export or an SMB
//! share `/etc/fstab` names, which the user may mount themselves (its
//! options say `user` or `users`), mounted with `mount <folder>`; and the
//! network mounts the system made (fstab, autofs), shown and named, let
//! go of only where the user may. Linux only for now; macOS and Windows
//! mount shares their own way.

use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Arc;
use std::time::Duration;

use skwir_core::cancel::CancelToken;
use skwir_core::fs::reach::Remote;

use crate::address::{Address, Scheme};
use crate::connector::{Ask, Connected, Connector, Recognised, Server};

/// How long the system may take to mount a share.
const PATIENCE: Duration = Duration::from_secs(60);

/// A network entry of fstab.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Entry {
    pub source: String,
    pub point: PathBuf,
    pub fstype: String,
    /// The user may mount it (`user`, `users`), and let go of it.
    pub users: bool,
    pub address: Option<Address>,
}

/// The network entries of `fstab`'s text.
pub fn entries(fstab: &str) -> Vec<Entry> {
    fstab
        .lines()
        .filter_map(|line| {
            // A comment is a line of its own: `sshfs#host:` is a source.
            let line = line.trim();
            if line.starts_with('#') {
                return None;
            }
            let mut fields = line.split_whitespace();
            let (source, point, fstype) = (fields.next()?, fields.next()?, fields.next()?);
            let options = fields.next().unwrap_or("defaults");
            let source = unescape(source);
            let address = address_of(fstype, &source)?;
            Some(Entry {
                users: options.split(',').any(|o| matches!(o, "user" | "users")),
                point: PathBuf::from(unescape(point)),
                fstype: fstype.to_owned(),
                source,
                address: Some(address),
            })
        })
        .collect()
}

/// fstab writes a space as `\040`.
fn unescape(field: &str) -> String {
    let mut out = String::with_capacity(field.len());
    let mut chars = field.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '\\' {
            let code: String = chars.clone().take(3).collect();
            if code.len() == 3
                && let Ok(byte) = u8::from_str_radix(&code, 8)
            {
                out.push(char::from(byte));
                for _ in 0..3 {
                    chars.next();
                }
                continue;
            }
        }
        out.push(c);
    }
    out
}

/// The address of a network mount by its type and source: `nas:/export`
/// for NFS, `//nas/share` for SMB, `me@host:/path` for sshfs.
pub fn address_of(fstype: &str, source: &str) -> Option<Address> {
    match fstype {
        "nfs" | "nfs4" => {
            let (host, path) = source.split_once(':')?;
            Address::parse(&format!("nfs://{host}{path}")).ok()
        }
        "cifs" | "smb3" | "smbfs" => {
            let rest = source.strip_prefix("//")?;
            Address::parse(&format!("smb://{rest}")).ok()
        }
        "fuse.sshfs" | "sshfs" => crate::sshfs::of_source(source.trim_start_matches("sshfs#")),
        _ => None,
    }
}

fn read_fstab() -> String {
    std::fs::read_to_string("/etc/fstab").unwrap_or_default()
}

pub struct Fstab;

impl Connector for Fstab {
    fn id(&self) -> &'static str {
        "fstab"
    }

    fn title(&self) -> &'static str {
        "the system's mounts"
    }

    fn schemes(&self) -> &'static [Scheme] {
        &[Scheme::Nfs, Scheme::Smb, Scheme::Sftp]
    }

    fn unavailable(&self) -> Option<String> {
        (!cfg!(target_os = "linux")).then(|| "fstab is read on Linux only".to_owned())
    }

    fn connect(
        &self,
        address: &Address,
        _ask: &Arc<dyn Ask>,
        cancel: &CancelToken,
    ) -> anyhow::Result<Connected> {
        let entry = entries(&read_fstab())
            .into_iter()
            .find(|e| {
                e.users
                    && e.address
                        .as_ref()
                        .is_some_and(|mounted| mounted.below(address).is_some())
            })
            .ok_or_else(|| anyhow::anyhow!("no entry of /etc/fstab that you may mount names it"))?;
        let mut command = Command::new("mount");
        command.arg(&entry.point);
        let finished = skwir_platform::process::run_to_exit(&mut command, None, cancel, PATIENCE)?;
        skwir_core::fs::reach::changed();
        anyhow::ensure!(
            finished.status.success() || finished.stderr.contains("already mounted"),
            "{}",
            finished.stderr.trim()
        );
        Ok(Connected {
            point: entry.point,
            address: entry.address.expect("filtered"),
        })
    }

    fn recognises(&self, remote: &Remote) -> Option<Recognised> {
        let source = remote.source.to_string_lossy();
        let address = address_of(&remote.fstype, &source);
        // A mount of the system's own, named as its folder is.
        address.as_ref()?;
        Some(Recognised {
            label: remote.point.file_name().map_or_else(
                || source.clone().into_owned(),
                |n| n.to_string_lossy().into_owned(),
            ),
            address,
            ours: false,
            device: None,
        })
    }

    fn disconnect(
        &self,
        remote: &Remote,
        _recognised: &Recognised,
        cancel: &CancelToken,
    ) -> anyhow::Result<()> {
        anyhow::ensure!(
            user_mountable(&remote.point),
            "the system mounted it: only an administrator may let go of it"
        );
        let umount = |lazy: bool| -> anyhow::Result<()> {
            let mut command = Command::new("umount");
            if lazy {
                command.arg("-l");
            }
            command.arg(&remote.point);
            let finished =
                skwir_platform::process::run_to_exit(&mut command, None, cancel, PATIENCE)?;
            skwir_core::fs::reach::changed();
            if finished.status.success() {
                Ok(())
            } else if finished.stderr.contains("busy") {
                Err(Busy.into())
            } else {
                anyhow::bail!("{}", finished.stderr.trim())
            }
        };
        match umount(false) {
            Err(error) if error.is::<Busy>() => {
                crate::mounter::when_busy(&remote.point, || umount(true))
            }
            ended => ended,
        }
    }

    fn servers(&self, _cancel: &CancelToken) -> Vec<Server> {
        entries(&read_fstab())
            .into_iter()
            .filter(|e| e.users)
            .filter_map(|e| {
                Some(Server {
                    label: e
                        .point
                        .file_name()
                        .map_or_else(|| e.source.clone(), |n| n.to_string_lossy().into_owned()),
                    address: e.address?,
                    kind: String::new(),
                    kept: crate::servers::Kept::Tool("in the system's fstab"),
                })
            })
            .collect()
    }
}

/// `umount` refused as busy.
#[derive(Debug)]
struct Busy;

impl std::fmt::Display for Busy {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("another program is using it")
    }
}

impl std::error::Error for Busy {}

/// Whether fstab lets the user mount and let go of what is at `point`.
pub fn user_mountable(point: &Path) -> bool {
    entries(&read_fstab())
        .iter()
        .any(|e| e.users && e.point == point)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fstabs_network_entries_are_read_with_what_the_user_may_do() {
        let fstab = "\
# the root
UUID=0000 / btrfs defaults 0 0
nas:/export/photos /mnt/photos nfs noauto,user,_netdev 0 0
//nas/music\\040box /mnt/music\\040box cifs credentials=/etc/smb,uid=1000 0 0
sshfs#me@example.com:/srv /mnt/srv fuse.sshfs users,noauto 0 0
";
        let found = entries(fstab);
        assert_eq!(found.len(), 3);
        assert_eq!(found[0].point, Path::new("/mnt/photos"));
        assert!(found[0].users);
        assert_eq!(
            found[0].address.as_ref().unwrap().to_string(),
            "nfs://nas/export/photos"
        );
        assert_eq!(found[1].point, Path::new("/mnt/music box"));
        assert!(!found[1].users);
        assert_eq!(
            found[1].address.as_ref().unwrap().to_string(),
            "smb://nas/music box"
        );
        assert_eq!(
            found[2].address.as_ref().unwrap().to_string(),
            "sftp://me@example.com/srv"
        );
    }
}
