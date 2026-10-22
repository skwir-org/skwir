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

/// What a plugin knows of a remote that the mount table does not say
/// (ADR-077): its address, its name, what is shown of it unasked. Kept by
/// its mount point in `Registry::remotes`.
#[derive(Clone, Debug, PartialEq)]
pub struct Known {
    /// Where its paths start (`Remote::point`).
    pub point: PathBuf,
    /// Its address (`sftp://me@example.com/`): what a folder there is
    /// kept as (tabs, the session), and opened by again once let go of.
    pub address: Option<crate::location::Location>,
    /// What the user calls it: `me@example.com`, `photos on nas`.
    pub label: String,
    pub thumbnails: Thumbnails,
    /// What its program is told before a folder of it is read again, where
    /// it answers from what it read before (kio-fuse keeps a listing and a
    /// file's attributes 30 seconds): `App::read_again`.
    pub forget: Option<Forget>,
}

/// What a remote's program is told so that it reads its server again
/// (`Known::forget`). Called in the remote's lane: it may wait on the
/// program.
#[derive(Clone)]
pub struct Forget(pub Arc<dyn Fn() + Send + Sync>);

impl std::fmt::Debug for Forget {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Forget")
    }
}

impl PartialEq for Forget {
    fn eq(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.0, &other.0)
    }
}

/// Which files of a remote show a thumbnail unasked, which reads them
/// across the network: the user's choice for it, else what its plugin
/// found (Always on the local network). A remote no plugin knows shows
/// none.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Thumbnails {
    #[default]
    Never,
    /// Files up to [`Thumbnails::SMALL`].
    Small,
    Always,
}

impl Thumbnails {
    /// The largest file a remote whose thumbnails are Small files shows
    /// one of: a phone's photograph.
    pub const SMALL: u64 = 4 << 20;

    /// Whether a file of `size` bytes (`None`: not known yet) shows one.
    pub fn shows(self, size: Option<u64>) -> bool {
        match self {
            Thumbnails::Never => false,
            Thumbnails::Small => size.is_some_and(|size| size <= Thumbnails::SMALL),
            Thumbnails::Always => true,
        }
    }
}

/// The remote holding `path`, if one does: by the path's text, so a path
/// reached through a link elsewhere is told by its folder's device once
/// opened ([`of_device`]).
pub fn of(path: &Path) -> Option<Arc<Remote>> {
    #[cfg(test)]
    if let Some(point) = PRETENDED_REMOTES
        .lock()
        .iter()
        .find(|point| path.starts_with(point))
    {
        return Some(Arc::new(Remote {
            point: point.clone(),
            fstype: "fuse.sshfs".to_owned(),
            source: "me@example.com:/".into(),
            device: None,
        }));
    }
    table().of(path)
}

/// The folders a test says remotes are mounted at ([`pretend_remote`]).
#[cfg(test)]
static PRETENDED_REMOTES: Mutex<Vec<PathBuf>> = Mutex::new(Vec::new());

/// Say, for this test's process (nextest runs each in one of its own),
/// that an sshfs remote is mounted at `point`: [`of`] says so of every
/// path below it.
#[cfg(all(test, target_os = "linux"))]
pub(crate) fn pretend_remote(point: &Path) {
    PRETENDED_REMOTES.lock().push(point.to_path_buf());
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

/// The number of the mount holding `device`'s files where its program
/// makes their inode numbers up for as long as it is mounted (FUSE:
/// sshfs, GVfs, rclone, and a FUSE mount on this machine alike), which
/// tells a file's made-up number apart (`fs::stat::on_mount`); 0
/// elsewhere. The system's
/// unique number for the mount where it says one (Linux 6.8), which no
/// later mount takes again; else its number in the table, which one may.
pub fn mount_of(device: u64) -> u64 {
    #[cfg(test)]
    if let Some(number) = PRETENDED.with(|p| {
        p.borrow()
            .iter()
            .find(|(of, _)| *of == device)
            .map(|&(_, number)| number)
    }) {
        return number;
    }
    // FUSE mounts on no block device of its own: a device that has one
    // (a disk's, ntfs-3g's) is told without the table.
    #[cfg(target_os = "linux")]
    if rustix::fs::major(device) != 0 {
        return 0;
    }
    table()
        .made_up
        .iter()
        .find(|(of, _)| *of == device)
        .map_or(0, |&(_, number)| number)
}

#[cfg(test)]
thread_local! {
    /// The devices a test says a mount's program numbers ([`pretend_made_up`]).
    static PRETENDED: std::cell::RefCell<Vec<(u64, u64)>> = const { std::cell::RefCell::new(Vec::new()) };
}

/// Say, for this thread's test, that `device`'s inode numbers are made up
/// by the mount whose number is `number` ([`mount_of`]).
#[cfg(test)]
pub(crate) fn pretend_made_up(device: u64, number: u64) {
    PRETENDED.with(|p| p.borrow_mut().push((device, number)));
}

/// Every network mount of the table, a bridge as one ([`bridged`] lists
/// what it holds).
pub fn all() -> Vec<Arc<Remote>> {
    table().remotes.clone()
}

/// The mount points of type `fstype` (a bridge: GVfs's, `fuse.gvfsd-fuse`;
/// kio-fuse's, `fuse.kio-fuse`).
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

/// The network mounts, deepest point first, and the devices whose inode
/// numbers are made up, each with its mount's number ([`mount_of`]).
#[derive(Default)]
struct Table {
    remotes: Vec<Arc<Remote>>,
    made_up: Vec<(u64, u64)>,
}

impl Table {
    fn read() -> Table {
        let (remotes, made_up) = imp::read();
        let mut remotes: Vec<Arc<Remote>> = remotes.into_iter().map(Arc::new).collect();
        remotes.sort_by_key(|r| std::cmp::Reverse(r.point.as_os_str().len()));
        Table { remotes, made_up }
    }

    fn of(&self, path: &Path) -> Option<Arc<Remote>> {
        #[cfg(windows)]
        if let Some(share) = imp::share_of(path) {
            return Some(Arc::new(share));
        }
        let remote = self.remotes.iter().find(|r| path.starts_with(&r.point))?;
        if let Some(depth) = bridge_depth(remote)
            && let Ok(rest) = path.strip_prefix(&remote.point)
        {
            // A bridge: each of its folders so deep is a mount of its own
            // (a folder above one, kio-fuse's `smb`, is its own too).
            let mut point = remote.point.clone();
            for part in rest.components().take(depth) {
                point.push(part);
            }
            if point != remote.point {
                return Some(Arc::new(Remote {
                    point,
                    device: None,
                    ..Remote::clone(remote)
                }));
            }
        }
        Some(Arc::clone(remote))
    }
}

/// The FUSE programs whose one mount holds many, each a folder so deep:
/// GVfs's bridge (`sftp:host=h`), kio-fuse (`smb/nas`, `ftp/me@host:2121`).
const BRIDGES: &[(&str, usize)] = &[("fuse.gvfsd-fuse", 1), ("fuse.kio-fuse", 2)];

/// How deep the mounts the table's mount `remote` holds are, when it is a
/// bridge ([`BRIDGES`]).
fn bridge_depth(remote: &Remote) -> Option<usize> {
    // A bridge is the table's own entry; a mount it holds is not one.
    remote.device?;
    BRIDGES
        .iter()
        .find(|(fstype, _)| *fstype == remote.fstype)
        .map(|(_, depth)| *depth)
}

/// The mounts the bridge `remote` holds ([`BRIDGES`]), each a remote of
/// its own; `remote` itself when it is no bridge. Lists the bridge's
/// folders, which its program answers from what it holds, never from a
/// server: on a worker.
pub fn bridged(remote: &Arc<Remote>) -> Vec<Arc<Remote>> {
    let Some(depth) = bridge_depth(remote) else {
        return vec![Arc::clone(remote)];
    };
    let mut level = vec![remote.point.clone()];
    for _ in 0..depth {
        level = level
            .iter()
            .filter_map(|folder| std::fs::read_dir(folder).ok())
            .flat_map(|folders| folders.flatten().map(|entry| entry.path()))
            .collect();
    }
    level.iter().filter_map(|point| table().of(point)).collect()
}

/// The FUSE types that reach a network. A FUSE type not named here is a
/// network's when what it mounted names a server (`host:/path`).
const FUSE_NETWORK: &[&str] = &[
    "sshfs",
    "rclone",
    "gvfsd-fuse",
    "kio-fuse",
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
    use std::path::Path;

    use super::Remote;
    use crate::fs::volume::Mounted;

    pub(super) fn read() -> (Vec<Remote>, Vec<(u64, u64)>) {
        let Ok(text) = std::fs::read("/proc/self/mountinfo") else {
            return (Vec::new(), Vec::new());
        };
        let mounts = crate::fs::volume::mountinfo(&text);
        (remotes(&mounts), made_up(&mounts, unique))
    }

    /// The network mounts of `mounts`.
    pub(super) fn remotes(mounts: &[Mounted]) -> Vec<Remote> {
        mounts
            .iter()
            .filter(|m| super::network(&m.fstype, &m.source))
            .map(|m| Remote {
                point: m.point.clone(),
                fstype: m.fstype.clone(),
                source: m.source.clone(),
                device: m.device,
            })
            .collect()
    }

    /// Each FUSE mount's device and its number: `unique`'s for its point,
    /// else the table's. `fuseblk` (ntfs-3g) keeps its volume's numbers.
    /// A point below another FUSE mount or a network's is not asked: its
    /// path would be looked up through that mount's program or server,
    /// which may not answer.
    pub(super) fn made_up(
        mounts: &[Mounted],
        unique: impl Fn(&Path) -> Option<u64>,
    ) -> Vec<(u64, u64)> {
        let fuse = |fstype: &str| fstype == "fuse" || fstype.starts_with("fuse.");
        let held: Vec<&Path> = mounts
            .iter()
            .filter(|m| fuse(&m.fstype) || super::network(&m.fstype, &m.source))
            .map(|m| m.point.as_path())
            .collect();
        mounts
            .iter()
            .filter(|m| fuse(&m.fstype))
            .filter_map(|m| {
                let below = held
                    .iter()
                    .any(|point| *point != m.point && m.point.starts_with(point));
                let number = (!below).then(|| unique(&m.point)).flatten().or(m.id)?;
                Some((m.device?, number.max(1)))
            })
            .collect()
    }

    /// The unique number of the mount at `point` (Linux 6.8), asked of
    /// what the kernel holds of it: `AT_STATX_DONT_SYNC`, and nothing
    /// asked that its program would be asked for.
    pub(super) fn unique(point: &Path) -> Option<u64> {
        use rustix::fs::{AtFlags, StatxFlags};
        /// `STATX_MNT_ID_UNIQUE`.
        const UNIQUE: u32 = 0x4000;
        let st = rustix::fs::statx(
            rustix::fs::CWD,
            point,
            AtFlags::STATX_DONT_SYNC | AtFlags::NO_AUTOMOUNT | AtFlags::SYMLINK_NOFOLLOW,
            StatxFlags::from_bits_retain(UNIQUE),
        )
        .ok()?;
        (st.stx_mask & UNIQUE != 0).then_some(st.stx_mnt_id)
    }
}

#[cfg(target_os = "macos")]
mod imp {
    use std::os::unix::ffi::OsStrExt;

    use super::Remote;

    /// macOS's mounts that are not local (`MNT_LOCAL` unset); no device
    /// is told whose numbers are made up.
    pub(super) fn read() -> (Vec<Remote>, Vec<(u64, u64)>) {
        (remotes(), Vec::new())
    }

    /// macOS's mounts that are not local (`MNT_LOCAL` unset), read by
    /// `getfsstat` without waiting on any (`MNT_NOWAIT`).
    fn remotes() -> Vec<Remote> {
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
    pub(super) fn read() -> (Vec<Remote>, Vec<(u64, u64)>) {
        (Vec::new(), Vec::new())
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
    pub(super) fn read() -> (Vec<super::Remote>, Vec<(u64, u64)>) {
        (Vec::new(), Vec::new())
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
            remotes: imp::remotes(&crate::fs::volume::mountinfo(text))
                .into_iter()
                .map(Arc::new)
                .collect(),
            made_up: Vec::new(),
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

    #[cfg(target_os = "linux")]
    #[test]
    fn each_server_kio_fuse_holds_is_a_remote_of_its_own() {
        let text = b"52 25 0:95 / /run/user/1000/kio-fuse-AbCdEf rw,nosuid,nodev shared:930 - fuse.kio-fuse kio-fuse rw,user_id=1000\n";
        let table = Table {
            remotes: imp::remotes(&crate::fs::volume::mountinfo(text))
                .into_iter()
                .map(Arc::new)
                .collect(),
            made_up: Vec::new(),
        };
        let root = Path::new("/run/user/1000/kio-fuse-AbCdEf");
        let share = table.of(&root.join("smb/nas/photos/2024")).unwrap();
        assert_eq!(
            share.point,
            root.join("smb/nas"),
            "a server, its shares below"
        );
        assert!(!share.by_kernel());
        let ftp = table.of(&root.join("ftp/me@host:2121/pub")).unwrap();
        assert_eq!(ftp.point, root.join("ftp/me@host:2121"));
        assert_ne!(
            share.lane(),
            ftp.lane(),
            "a server that stops holds its own"
        );
        assert_eq!(table.of(&root.join("smb")).unwrap().point, root.join("smb"));
        assert_eq!(table.of(root).unwrap().point, root);
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn a_mount_that_makes_its_numbers_up_is_told_by_its_own() {
        let text =
            b"36 25 0:88 / /run/user/1000/gvfs rw shared:922 - fuse.gvfsd-fuse gvfsd-fuse rw\n\
37 25 0:98 / /home/me/srv rw shared:9 - fuse.sshfs me@host:/ rw\n\
38 37 0:99 / /home/me/srv/inner rw shared:10 - fuse.sshfs me@other:/ rw\n\
39 25 0:91 / /mnt/nas rw shared:11 - nfs4 nas:/export rw\n\
40 39 0:92 / /mnt/nas/x rw shared:12 - fuse.portal portal rw\n\
41 25 8:17 / /mnt/win rw shared:13 - fuseblk /dev/sdb1 rw\n\
42 25 259:2 / / rw shared:1 - btrfs /dev/nvme0n1p2 rw\n";
        let mounts = crate::fs::volume::mountinfo(text);
        let asked = parking_lot::Mutex::new(Vec::new());
        let made_up = imp::made_up(&mounts, |point| {
            asked.lock().push(point.to_path_buf());
            (point != Path::new("/home/me/srv")).then_some(1 << 40)
        });
        let dev = |minor| rustix::fs::makedev(0, minor);
        assert_eq!(
            made_up,
            [
                (dev(88), 1 << 40),
                (dev(98), 37),
                (dev(99), 38),
                (dev(92), 40)
            ],
            "the unique number where the system says it, else the table's"
        );
        assert_eq!(
            *asked.lock(),
            [Path::new("/run/user/1000/gvfs"), Path::new("/home/me/srv")],
            "a point below a FUSE mount or a network's is not looked up"
        );
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn a_local_device_makes_no_number_up() {
        let root = crate::fs::stat::id_of(Path::new("/")).expect("the root");
        assert_eq!(mount_of(root.dev), 0);
        assert!(root.lasts());
        let release = std::fs::read_to_string("/proc/sys/kernel/osrelease").unwrap();
        let mut parts = release
            .split(['.', '-'])
            .map(|n| n.parse::<u32>().unwrap_or(0));
        if (parts.next().unwrap_or(0), parts.next().unwrap_or(0)) >= (6, 8) {
            assert!(imp::unique(Path::new("/")).is_some(), "Linux 6.8 says it");
        }
    }
}
