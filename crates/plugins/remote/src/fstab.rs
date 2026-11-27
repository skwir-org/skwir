//! The system's own network mounts (ADR-077): an NFS export or an SMB
//! share `/etc/fstab` names, which the user may mount themselves (its
//! options say `user` or `users`), mounted with `mount <folder>`; and the
//! network mounts the system made (fstab, autofs), shown and named, let
//! go of only where the user may. Linux only for now; macOS and Windows
//! mount shares their own way.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use skwir_core::cancel::CancelToken;
use skwir_core::fs::reach::Remote;
use skwir_core::words;
use skwir_core::words::Words;

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

/// The system's table of file systems to mount.
const FSTAB: &str = "/etc/fstab";

fn read_fstab() -> String {
    std::fs::read_to_string(FSTAB).unwrap_or_default()
}

pub struct Fstab;

impl Connector for Fstab {
    fn id(&self) -> &'static str {
        "fstab"
    }

    fn title(&self) -> Words {
        words!(
            remote_fstab = "the system's mounts",
            note = "What mounts the network folders the system's table (fstab) names: a tool's \
                    name, as sshfs and GVfs are, said before what it could not do."
        )
    }

    fn schemes(&self) -> &'static [Scheme] {
        &[Scheme::Nfs, Scheme::Smb, Scheme::Sftp]
    }

    fn unavailable(&self) -> Option<Words> {
        (!cfg!(target_os = "linux"))
            .then(|| words!(remote_fstab_linux_only = "fstab is read on Linux only"))
    }

    fn connect(
        &self,
        address: &Address,
        _ask: &Arc<dyn Ask>,
        cancel: &CancelToken,
    ) -> anyhow::Result<Connected> {
        let (entry, point) = mountable()
            .into_iter()
            .find(|(e, _)| {
                e.address
                    .as_ref()
                    .is_some_and(|mounted| mounted.below(address).is_some())
            })
            .ok_or_else(|| {
                anyhow::anyhow!(words!(
                    remote_fstab_no_entry = "no entry of /etc/fstab that you may mount names it"
                ))
            })?;
        let mut command = skwir_platform::process::command("mount");
        command.arg(&entry.point);
        let finished = skwir_platform::process::run_to_exit(&mut command, None, cancel, PATIENCE)?;
        skwir_core::fs::reach::changed();
        anyhow::ensure!(
            finished.status.success() || finished.stderr.contains("already mounted"),
            "{}",
            finished.stderr.trim()
        );
        // `mount` may end before its mount is there: an NFS entry with `bg`
        // goes on trying in the background once its server did not answer
        // at once, and ends saying it succeeded, nothing mounted; an sshfs
        // entry's daemon answers before the kernel lists it. Waited for as
        // every connector waits (`connector::appears`): the point handed back
        // with nothing mounted there would show the empty folder below it
        // as the server's. Looked for where the table lists it (`mountable`).
        let mounted = crate::connector::appears(cancel, || {
            skwir_core::fs::reach::of(&point).filter(|remote| remote.point == point)
        });
        anyhow::ensure!(
            mounted.is_some() || !cancel.is_cancelled(),
            crate::connector::cancelled()
        );
        anyhow::ensure!(
            mounted.is_some(),
            words!(
                remote_fstab_not_mounted =
                    "the system has not mounted { $folder }: it may still be trying",
                folder = entry.point.display().to_string(),
                note = "Why a network folder of the system's table (fstab) did not open: its mount command ended, but nothing is mounted at the folder yet (an NFS mount the system goes on trying in the background). folder: where it mounts, a path."
            )
        );
        Ok(Connected {
            point,
            address: entry.address.expect("filtered"),
        })
    }

    fn recognises(&self, remote: &Remote) -> Option<Recognised> {
        let source = remote.source.to_string_lossy();
        let address = address_of(&remote.fstype, &source);
        // A mount of the system's own, named as its folder is.
        address.as_ref()?;
        Some(Recognised {
            label: Words::as_written(remote.point.file_name().map_or_else(
                || source.clone().into_owned(),
                |n| n.to_string_lossy().into_owned(),
            )),
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
            words!(
                remote_fstab_administrator =
                    "the system mounted it: only an administrator may let go of it"
            )
        );
        let umount = |lazy: bool| -> anyhow::Result<()> {
            let mut command = skwir_platform::process::command("umount");
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

    fn servers(&self, _cancel: &CancelToken) -> crate::connector::Named {
        let servers = entries(&read_fstab())
            .into_iter()
            .filter(|e| e.users)
            .filter_map(|e| {
                Some(Server {
                    label: Words::as_written(
                        e.point
                            .file_name()
                            .map_or_else(|| e.source.clone(), |n| n.to_string_lossy().into_owned()),
                    ),
                    address: e.address?,
                    kind: String::new(),
                    kept: crate::servers::Kept::Tool(crate::servers::Keeper::Fstab),
                })
            })
            .collect();
        crate::connector::Named {
            servers,
            from: vec![crate::connector::Source::File(PathBuf::from(FSTAB))],
        }
    }
}

/// `umount` refused as busy.
#[derive(Debug)]
struct Busy;

impl std::fmt::Display for Busy {
    /// Its words: in the reader's language while an error's chain is said.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        std::fmt::Display::fmt(&crate::mounter::used_by_another(), f)
    }
}

impl std::error::Error for Busy {}

/// Whether fstab lets the user mount and let go of what is at `point`, a
/// point as the mount table lists it ([`mountable`]). Reads fstab and
/// what its points lead through: on the thread that reads the table
/// (`Remotes::mounted_with`), never while the plugin's state is locked.
pub fn user_mountable(point: &Path) -> bool {
    mountable().iter().any(|(_, listed)| listed == point)
}

/// The network entries of fstab the user may mount (`user`, `users`),
/// each with where the mount table lists a mount made at its point: the
/// point named as the table names it, read as the entries are
/// (`reach::as_listed`: `mount` and the kernel mount where a link leads,
/// the point's own too, and Fedora's atomic desktops link `/mnt` to
/// `/var/mnt`), never through a mount there or on a remote. I/O: fstab,
/// and a stat each part of each point, on a worker.
fn mountable() -> Vec<(Entry, PathBuf)> {
    mountable_of(&read_fstab())
}

/// [`mountable`], fstab's text being `fstab`.
fn mountable_of(fstab: &str) -> Vec<(Entry, PathBuf)> {
    entries(fstab)
        .into_iter()
        .filter(|e| e.users)
        .map(|e| {
            let listed = skwir_core::fs::reach::as_listed(&e.point);
            (e, listed)
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// What the user may mount is looked for where the table lists it:
    /// a point that is a link, or below one, where it leads, as `mount`
    /// mounts it; what the user may not mount is not offered.
    #[cfg(unix)]
    #[test]
    fn a_point_is_looked_for_where_the_table_lists_it() {
        let dir = tempfile::tempdir().unwrap();
        let root = skwir_core::os_path::canonical(dir.path()).unwrap();
        let srv = root.join("srv").join("nas");
        let var_mnt = root.join("var").join("mnt");
        std::fs::create_dir_all(&srv).unwrap();
        std::fs::create_dir_all(var_mnt.join("photos")).unwrap();
        std::os::unix::fs::symlink(&srv, root.join("nas")).unwrap();
        std::os::unix::fs::symlink(&var_mnt, root.join("mnt")).unwrap();
        let fstab = format!(
            "nas:/export {root}/nas nfs user,noauto 0 0\n\
             nas:/photos {root}/mnt/photos nfs users,noauto 0 0\n\
             nas:/admin {root}/mnt/admin nfs noauto 0 0\n",
            root = root.display()
        );
        let found = mountable_of(&fstab);
        assert_eq!(
            found
                .iter()
                .map(|(e, listed)| (e.point.clone(), listed.clone()))
                .collect::<Vec<_>>(),
            [
                (root.join("nas"), srv),
                (root.join("mnt").join("photos"), var_mnt.join("photos"))
            ]
        );
    }

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
