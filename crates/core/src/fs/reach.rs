//! Whether a path is on this machine or reached over a network (ADR-077).
//!
//! A remote folder is a folder: a mount an installed tool made (sshfs,
//! GVfs, rclone, the kernel's NFS and CIFS, Windows' shares). What differs
//! there is told here, from the system's table of mounts, never by reading
//! the remote: where work on it runs ([`lane`], so a server that stops
//! answering holds its own jobs, never the window's or the local disks'),
//! and what is not done there unasked.
//!
//! The table is read again when older than [`FRESH`], and at once after a
//! mount made or let go of here ([`changed`]). Asked from any thread: a
//! lookup is a search in memory.

use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use parking_lot::Mutex;

use crate::scheduler::Lane;

/// How long the table read is taken as the system's.
pub const FRESH: Duration = Duration::from_secs(1);

/// A mount that reaches a network.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Remote {
    /// Where its paths start. For GVfs, whose FUSE bridge holds every
    /// mount of the session as a folder of its own, that folder.
    pub point: PathBuf,
    /// Its type as the system says it: `nfs4`, `cifs`, `fuse.sshfs`.
    pub fstype: String,
    /// What was mounted: `nas:/export`, `//nas/photos`, `me@host:/`.
    pub source: OsString,
    /// The device its files are on (Linux), which a folder opened through
    /// a link to it tells.
    pub device: Option<u64>,
}

impl Remote {
    /// Whether the kernel speaks to the server itself (NFS, SMB, 9p):
    /// inodes are the server's, and a share keeps a trash of its own as
    /// freedesktop says. A FUSE remote's inodes are made up by its program
    /// for as long as it is mounted, and it keeps no trash Skwir puts
    /// files in.
    pub fn by_kernel(&self) -> bool {
        !self.fstype.starts_with("fuse")
    }

    /// The lane its work runs in.
    pub fn lane(&self) -> Lane {
        Lane::keyed(key_of(&self.point))
    }
}

/// The remote holding `path`, if one does: by the path's text, so a path
/// reached through a link elsewhere is told by its folder's device once
/// opened ([`of_device`]).
pub fn of(path: &Path) -> Option<Arc<Remote>> {
    table().of(path)
}

/// The remote whose files are on `device` (a stat's), if one is.
pub fn of_device(device: u64) -> Option<Arc<Remote>> {
    table()
        .remotes
        .iter()
        .find(|r| r.device == Some(device))
        .cloned()
}

/// The lane of work on `path`: its remote's, the shared one elsewhere.
pub fn lane(path: &Path) -> Lane {
    of(path).map_or(Lane::SHARED, |remote| remote.lane())
}

/// Every network mount of the table, each GVfs mount apart (its bridge
/// listed, read on the caller's thread: on a worker).
pub fn all() -> Vec<Arc<Remote>> {
    table().remotes.clone()
}

/// The mount points of type `fstype` (GVfs's bridge, `fuse.gvfsd-fuse`).
pub fn bridges(fstype: &str) -> Vec<PathBuf> {
    table()
        .remotes
        .iter()
        .filter(|r| r.fstype == fstype && r.device.is_some())
        .map(|r| r.point.clone())
        .collect()
}

/// Whether `path` is where a network mount starts: what a walk does not
/// enter from a folder above it.
pub fn is_point(path: &Path) -> bool {
    table().remotes.iter().any(|r| r.point == path)
}

/// Forget the table read: a mount was made or let go of here.
pub fn changed() {
    *TABLE.lock() = None;
}

/// The table as last read, read again when stale.
fn table() -> Arc<Table> {
    let mut held = TABLE.lock();
    if let Some((read, table)) = &*held
        && read.elapsed() < FRESH
    {
        return Arc::clone(table);
    }
    let table = Arc::new(Table::read());
    *held = Some((Instant::now(), Arc::clone(&table)));
    table
}

static TABLE: Mutex<Option<(Instant, Arc<Table>)>> = Mutex::new(None);

/// The network mounts, deepest point first.
#[derive(Default)]
struct Table {
    remotes: Vec<Arc<Remote>>,
}

impl Table {
    fn read() -> Table {
        let mut remotes: Vec<Arc<Remote>> = imp::remotes().into_iter().map(Arc::new).collect();
        remotes.sort_by_key(|r| std::cmp::Reverse(r.point.as_os_str().len()));
        Table { remotes }
    }

    fn of(&self, path: &Path) -> Option<Arc<Remote>> {
        #[cfg(windows)]
        if let Some(share) = imp::share_of(path) {
            return Some(Arc::new(share));
        }
        let remote = self.remotes.iter().find(|r| path.starts_with(&r.point))?;
        if remote.fstype == GVFS && remote.device.is_some() {
            // The bridge: each folder of it is a mount of its own.
            if let Some(first) = path
                .strip_prefix(&remote.point)
                .ok()
                .and_then(|rest| rest.components().next())
            {
                return Some(Arc::new(Remote {
                    point: remote.point.join(first),
                    device: None,
                    ..Remote::clone(remote)
                }));
            }
        }
        Some(Arc::clone(remote))
    }
}

/// GVfs's FUSE bridge.
const GVFS: &str = "fuse.gvfsd-fuse";

/// The FUSE types that reach a network. A FUSE type not named here is a
/// network's when what it mounted names a server (`host:/path`).
const FUSE_NETWORK: &[&str] = &[
    "sshfs",
    "rclone",
    "gvfsd-fuse",
    "curlftpfs",
    "s3fs",
    "gcsfuse",
    "goofys",
    "geesefs",
    "davfs",
    "davfs2",
    "smbnetfs",
    "afpfs",
    "afpfs-ng",
    "glusterfs",
    "ceph-fuse",
    "juicefs",
    "blobfuse",
    "blobfuse2",
    "mountpoint-s3",
    "ossfs",
    "onedriver",
    "google-drive-ocamlfuse",
    "seaweedfs",
    "cvmfs2",
    "httpdirfs",
    "httpfs2",
];

/// The kernel's network file systems.
const KERNEL_NETWORK: &[&str] = &[
    "nfs",
    "nfs4",
    "cifs",
    "smb3",
    "smbfs",
    "9p",
    "ceph",
    "afs",
    "coda",
    "ncpfs",
    "lustre",
    "glusterfs",
    "gpfs",
    "beegfs",
    "orangefs",
    "pvfs2",
    "davfs",
];

/// Whether a mount of `fstype` from `source` reaches a network.
pub fn network(fstype: &str, source: &std::ffi::OsStr) -> bool {
    if KERNEL_NETWORK.contains(&fstype) {
        return true;
    }
    let Some(subtype) = fstype.strip_prefix("fuse.") else {
        return false;
    };
    if FUSE_NETWORK.contains(&subtype) {
        return true;
    }
    // `me@host:/srv`, `host:path`: a server's, not a device's or a file's.
    let source = source.to_string_lossy();
    !source.starts_with('/')
        && source
            .split_once(':')
            .is_some_and(|(server, _)| !server.is_empty() && !server.contains('/'))
}

/// A key of `point`, the same each time: its lane's.
fn key_of(point: &Path) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for &b in point.as_os_str().as_encoded_bytes() {
        h = (h ^ u64::from(b)).wrapping_mul(0x0100_0000_01b3);
    }
    h
}

#[cfg(target_os = "linux")]
mod imp {
    use super::Remote;

    pub(super) fn remotes() -> Vec<Remote> {
        let Ok(text) = std::fs::read("/proc/self/mountinfo") else {
            return Vec::new();
        };
        of_mountinfo(&text)
    }

    /// The network mounts `mountinfo` lists.
    pub(super) fn of_mountinfo(text: &[u8]) -> Vec<Remote> {
        crate::fs::volume::mountinfo(text)
            .into_iter()
            .filter(|m| super::network(&m.fstype, &m.source))
            .map(|m| Remote {
                point: m.point,
                fstype: m.fstype,
                source: m.source,
                device: m.device,
            })
            .collect()
    }
}

#[cfg(target_os = "macos")]
mod imp {
    use std::os::unix::ffi::OsStrExt;

    use super::Remote;

    /// macOS's mounts that are not local (`MNT_LOCAL` unset), read by
    /// `getfsstat` without waiting on any (`MNT_NOWAIT`).
    pub(super) fn remotes() -> Vec<Remote> {
        fn name(chars: &[libc::c_char]) -> Vec<u8> {
            chars
                .iter()
                .map(|c| c.cast_unsigned())
                .take_while(|b| *b != 0)
                .collect()
        }
        // SAFETY: no buffer: the call only counts the mounts.
        #[allow(unsafe_code)]
        let count = unsafe { libc::getfsstat(std::ptr::null_mut(), 0, libc::MNT_NOWAIT) };
        let Ok(count) = usize::try_from(count) else {
            return Vec::new();
        };
        let mut table: Vec<libc::statfs> = Vec::with_capacity(count + 8);
        let Ok(bytes) = libc::c_int::try_from(table.capacity() * size_of::<libc::statfs>()) else {
            return Vec::new();
        };
        // SAFETY: the buffer has room for `bytes` bytes; the call writes
        // whole records within them and says how many.
        #[allow(unsafe_code)]
        let filled = unsafe { libc::getfsstat(table.as_mut_ptr(), bytes, libc::MNT_NOWAIT) };
        let Ok(filled) = usize::try_from(filled) else {
            return Vec::new();
        };
        // SAFETY: the call initialized the first `filled` records.
        #[allow(unsafe_code)]
        unsafe {
            table.set_len(filled.min(table.capacity()));
        }
        table
            .iter()
            .filter(|fs| fs.f_flags & (libc::MNT_LOCAL as u32) == 0)
            .map(|fs| Remote {
                point: std::ffi::OsStr::from_bytes(&name(&fs.f_mntonname)).into(),
                fstype: String::from_utf8_lossy(&name(&fs.f_fstypename)).into_owned(),
                source: std::ffi::OsStr::from_bytes(&name(&fs.f_mntfromname)).into(),
                device: None,
            })
            .collect()
    }
}

#[cfg(windows)]
mod imp {
    use std::path::{Component, Path, Prefix};

    use super::Remote;

    /// Windows lists no table: a path is told by its own form
    /// ([`share_of`]).
    pub(super) fn remotes() -> Vec<Remote> {
        Vec::new()
    }

    /// The share holding `path`: a UNC path's (`\\server\share`), or a
    /// drive letter mapped to one (`GetDriveTypeW`'s `DRIVE_REMOTE`).
    pub(super) fn share_of(path: &Path) -> Option<Remote> {
        let Some(Component::Prefix(prefix)) = path.components().next() else {
            return None;
        };
        match prefix.kind() {
            Prefix::UNC(server, share) | Prefix::VerbatimUNC(server, share) => {
                let mut point = std::ffi::OsString::from(r"\\");
                point.push(server);
                point.push(r"\");
                point.push(share);
                point.push(r"\");
                Some(Remote {
                    point: point.clone().into(),
                    fstype: "smb".to_owned(),
                    source: point,
                    device: None,
                })
            }
            Prefix::Disk(letter) | Prefix::VerbatimDisk(letter) => {
                /// `DRIVE_REMOTE`.
                const REMOTE: u32 = 4;
                let root: Vec<u16> =
                    [u16::from(letter), u16::from(b':'), u16::from(b'\\'), 0].to_vec();
                // SAFETY: a NUL-terminated wide string of a drive's root.
                #[allow(unsafe_code)]
                let kind = unsafe {
                    windows_sys::Win32::Storage::FileSystem::GetDriveTypeW(root.as_ptr())
                };
                (kind == REMOTE).then(|| {
                    let point = format!("{}:\\", char::from(letter));
                    Remote {
                        point: point.clone().into(),
                        fstype: "network drive".to_owned(),
                        source: point.into(),
                        device: None,
                    }
                })
            }
            _ => None,
        }
    }
}

#[cfg(not(any(target_os = "linux", target_os = "macos", windows)))]
mod imp {
    pub(super) fn remotes() -> Vec<super::Remote> {
        Vec::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn network_mounts_are_told_by_their_type_and_source() {
        let os = std::ffi::OsStr::new;
        assert!(network("nfs4", os("nas:/export")));
        assert!(network("cifs", os("//nas/photos")));
        assert!(network("fuse.sshfs", os("me@host:/")));
        assert!(network("fuse.rclone", os("gdrive:")));
        assert!(network("fuse.gvfsd-fuse", os("gvfsd-fuse")));
        assert!(
            network("fuse.someday", os("me@server:/srv")),
            "a server's source"
        );
        assert!(!network("fuseblk", os("/dev/sde1")), "ntfs-3g");
        assert!(!network("fuse.portal", os("portal")));
        assert!(!network("fuse.someday", os("/home/me/archive.zip")));
        assert!(!network("btrfs", os("/dev/nvme0n1p2")));
        assert!(!network("tmpfs", os("tmpfs")));
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn each_gvfs_mount_is_a_remote_of_its_own() {
        let text = b"36 25 0:88 / /run/user/1000/gvfs rw,nosuid shared:922 - fuse.gvfsd-fuse gvfsd-fuse rw,user_id=1000\n\
40 25 0:90 / /mnt/nas rw shared:9 - nfs4 nas:/export rw\n\
41 25 259:2 / / rw shared:1 - btrfs /dev/nvme0n1p2 rw\n";
        let table = Table {
            remotes: imp::of_mountinfo(text).into_iter().map(Arc::new).collect(),
        };
        assert_eq!(table.remotes.len(), 2);
        let sftp = table
            .of(Path::new("/run/user/1000/gvfs/sftp:host=h/home/me"))
            .unwrap();
        assert_eq!(sftp.point, Path::new("/run/user/1000/gvfs/sftp:host=h"));
        let smb = table
            .of(Path::new(
                "/run/user/1000/gvfs/smb-share:server=nas,share=s",
            ))
            .unwrap();
        assert_ne!(sftp.lane(), smb.lane(), "a server that stops holds its own");
        let nfs = table.of(Path::new("/mnt/nas/photos")).unwrap();
        assert_eq!(nfs.device, Some(rustix::fs::makedev(0, 90)));
        assert!(nfs.by_kernel());
        assert!(table.of(Path::new("/home/me")).is_none());
        assert!(table.of(Path::new("/mnt/nasty")).is_none(), "by component");
    }
}
