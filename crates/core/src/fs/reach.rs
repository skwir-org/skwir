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

/// How long a remote's lane runs a job before the remote is said not to
/// answer: in a pane's banner, and to a question asked of it that waits
/// no longer (ADR-077).
pub const NOT_ANSWERING: Duration = Duration::from_secs(3);

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

    /// Whether its program makes a file's change time up: kio-fuse gives
    /// the moment it read the file's attributes, anew each time (after 30
    /// seconds, or told to forget), never the server's. What compares a
    /// file with itself over time leaves it out there.
    pub fn makes_up_change_times(&self) -> bool {
        self.fstype == "fuse.kio-fuse"
    }

    /// Whether its program moves an entry from one of its folders to
    /// another: not GVfs's for a camera (gPhoto2, its bridge's folder
    /// `gphoto2:…`), nor Skwir's own for a phone KDE's kmtpd holds
    /// (`fuse.skwir-kmtp`, ADR-097), which rename within a folder alone.
    /// What is made there is written in place, never staged and moved
    /// (ADR-083).
    pub fn moves_between_folders(&self) -> bool {
        let camera = self.fstype == "fuse.gvfsd-fuse"
            && self
                .point
                .file_name()
                .is_some_and(|name| name.as_encoded_bytes().starts_with(b"gphoto2:"));
        !(camera || self.fstype == "fuse.skwir-kmtp")
    }
}

/// Whether what is made in the folder `dest` is written in place there,
/// its remote moving nothing between its folders
/// ([`Remote::moves_between_folders`], ADR-083). Asked of the table in
/// memory.
pub fn writes_in_place(dest: &Path) -> bool {
    of(dest).is_some_and(|remote| !remote.moves_between_folders())
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
    /// Where its server keeps what is deleted there, in the user's words
    /// (“Google Drive's trash”): a deletion's question says so rather than
    /// that it is gone for good (ADR-086). `None`: deleted for good, as on
    /// any network folder.
    pub trash: Option<String>,
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
    if let Some(remote) = pretended()
        .into_iter()
        .find(|remote| path.starts_with(&remote.point))
    {
        return Some(remote);
    }
    table().of(path)
}

/// The folders a test says remotes are mounted at, and their types
/// ([`pretend_remote`]).
#[cfg(test)]
static PRETENDED_REMOTES: Mutex<Vec<(PathBuf, &str)>> = Mutex::new(Vec::new());

/// The remotes a test says are mounted, as the table would list them:
/// every question asked of the table answers for them too ([`of`],
/// [`all`]), whatever this machine has mounted.
#[cfg(test)]
fn pretended() -> Vec<Arc<Remote>> {
    PRETENDED_REMOTES
        .lock()
        .iter()
        .map(|(point, fstype)| {
            Arc::new(Remote {
                point: point.clone(),
                fstype: (*fstype).to_owned(),
                source: "me@example.com:/".into(),
                device: None,
            })
        })
        .collect()
}

/// Say, for this test's process (nextest runs each in one of its own),
/// that an sshfs remote is mounted at `point`: [`of`] says so of every
/// path below it.
#[cfg(all(test, target_os = "linux"))]
pub(crate) fn pretend_remote(point: &Path) {
    PRETENDED_REMOTES
        .lock()
        .push((point.to_path_buf(), "fuse.sshfs"));
}

/// Say, for this test's process, that a camera GVfs mounted is at
/// `point`, a folder named as GVfs's are (`gphoto2:…`): what is made below
/// it is written in place ([`writes_in_place`]).
#[cfg(all(test, target_os = "linux"))]
pub(crate) fn pretend_camera(point: &Path) {
    debug_assert!(
        point
            .file_name()
            .is_some_and(|n| n.as_encoded_bytes().starts_with(b"gphoto2:"))
    );
    PRETENDED_REMOTES
        .lock()
        .push((point.to_path_buf(), "fuse.gvfsd-fuse"));
}

/// The remote whose files are on `device` (a stat's), if one is.
pub fn of_device(device: u64) -> Option<Arc<Remote>> {
    table()
        .remotes
        .iter()
        .find(|r| r.device == Some(device))
        .cloned()
}

/// Whether the mount at `point` answers: still mounted there (the table in
/// memory), how full it is asked of it and its top read, which a FUSE
/// program asks its server for (sshfs, GVfs) and the kernel's shares
/// theirs. Blocking while a server holds it: in its lane, one question at a
/// time.
pub fn answers(point: &Path) -> bool {
    if of(point).is_none_or(|remote| remote.point != point) {
        return false;
    }
    #[cfg(unix)]
    if rustix::fs::statvfs(point).is_err() {
        return false;
    }
    std::fs::read_dir(point).is_ok_and(|mut listed| listed.next().is_none_or(|e| e.is_ok()))
}

/// Reads of at least this many bytes say how fast a server is: a phone's
/// photograph, what Small files shows ([`Thumbnails::SMALL`]).
pub const PROVED_FROM: u64 = Thumbnails::SMALL;

/// Read at least this fast (bytes a second), a server proved fast: a
/// photograph of Small files in under half a second.
pub const FAST: f64 = 10_000_000.;

/// Read slower than this, a server proved fast proves slow again; between
/// the two, it stays as it was proved.
pub const SLOW: f64 = 5_000_000.;

/// What reads proved of each mount this session, fast or not: by its
/// point and its number (`mount_of`), so a mount made again is unknown
/// again. Kept in memory alone, as the plugin's word on it is.
static PROVED: Mutex<Option<std::collections::HashMap<(PathBuf, u64), bool>>> = Mutex::new(None);

/// Told each time what reads proved of a mount changed ([`proved_fast`]).
static PROVED_CHANGED: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

/// `bytes` read from `src` onto `into` in `took`: from a remote onto this
/// machine, it says how fast that server is (a copy within a server, or
/// onto another, says nothing of its reads), once enough was read
/// ([`PROVED_FROM`], [`FAST`], [`SLOW`]). What the user read anyway, never
/// a read made to find out.
pub fn note_read(src: &Path, into: &Path, bytes: u64, took: Duration) {
    if bytes < PROVED_FROM || took.is_zero() || of(into).is_some() {
        return;
    }
    let Some(remote) = of(src) else {
        return;
    };
    #[allow(clippy::cast_precision_loss)] // A speed, not a count.
    let speed = bytes as f64 / took.as_secs_f64();
    let key = (remote.point.clone(), mount_of(remote.device.unwrap_or(0)));
    let mut proved = PROVED.lock();
    let proved = proved.get_or_insert_with(Default::default);
    let was = proved.get(&key).copied();
    let now = if speed >= FAST {
        Some(true)
    } else if speed < SLOW {
        Some(false)
    } else {
        was
    };
    if let Some(now) = now
        && was != Some(now)
    {
        proved.insert(key, now);
        PROVED_CHANGED.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    }
}

/// Whether reads of `remote` this session proved it fast ([`note_read`]);
/// `None` before enough was read.
pub fn proved_fast(remote: &Remote) -> Option<bool> {
    let key = (remote.point.clone(), mount_of(remote.device.unwrap_or(0)));
    PROVED.lock().as_ref()?.get(&key).copied()
}

/// How often what reads proved changed: a plugin that words a remote by it
/// compares it with the count it last saw.
pub fn proved_changed() -> u64 {
    PROVED_CHANGED.load(std::sync::atomic::Ordering::Relaxed)
}

/// The lane of work on `path`: its remote's, the shared one elsewhere.
pub fn lane(path: &Path) -> Lane {
    of(path).map_or(Lane::SHARED, |remote| remote.lane())
}

/// `items` grouped by the lane of their path (`lane`), in the order first
/// met: what a location of files from anywhere (the shelf, a tag, Recent)
/// stats, a job for each lane, so a server that stopped answering holds
/// its own alone. From memory: nothing is read.
pub fn by_lane<T>(
    items: impl IntoIterator<Item = T>,
    path: impl Fn(&T) -> &Path,
) -> Vec<(Lane, Vec<T>)> {
    let mut lanes: Vec<(Lane, Vec<T>)> = Vec::new();
    for item in items {
        let lane = lane(path(&item));
        match lanes.iter_mut().find(|(l, _)| *l == lane) {
            Some((_, items)) => items.push(item),
            None => lanes.push((lane, vec![item])),
        }
    }
    lanes
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

/// The folders whose folders are mounts, each with its program's type,
/// where the system lists no mount table (Windows): Skwir's own there are
/// found so (`mounts_in`).
static MOUNT_FOLDERS: Mutex<Vec<(PathBuf, String)>> = Mutex::new(Vec::new());

/// Say that the folders of `folder` that are mount points (reparse
/// points, on Windows) are mounts of type `fstype`: where the system lists
/// no mount table (Windows), a program that mounts in a folder of its own
/// (rclone through `WinFsp`, ADR-086) is known so. Elsewhere, the table says.
pub fn mounts_in(folder: PathBuf, fstype: &str) {
    let mut folders = MOUNT_FOLDERS.lock();
    if !folders.iter().any(|(f, _)| *f == folder) {
        folders.push((folder, fstype.to_owned()));
    }
    drop(folders);
    changed();
}

/// Every network mount of the table, a bridge as one ([`bridged`] lists
/// what it holds).
pub fn all() -> Vec<Arc<Remote>> {
    #[cfg(test)]
    let pretended = pretended();
    #[cfg(not(test))]
    let pretended = Vec::new();
    pretended
        .into_iter()
        .chain(table().remotes.iter().cloned())
        .collect()
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
    // A phone KDE's kmtpd holds, mounted by Skwir's helper (ADR-097): a
    // device's, near, but waiting on the phone as a remote waits on its
    // server, so known here as GVfs's phones are.
    "skwir-kmtp",
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
    /// ([`share_of`]), and a program's mounts in a folder it said by their
    /// being mount points there ([`super::mounts_in`]).
    pub(super) fn read() -> (Vec<Remote>, Vec<(u64, u64)>) {
        use std::os::windows::fs::MetadataExt;
        /// `FILE_ATTRIBUTE_REPARSE_POINT`: what a folder a file system is
        /// mounted on is.
        const REPARSE_POINT: u32 = 0x400;
        let folders = super::MOUNT_FOLDERS.lock().clone();
        let mut remotes = Vec::new();
        for (folder, fstype) in folders {
            let Ok(entries) = std::fs::read_dir(&folder) else {
                continue;
            };
            for entry in entries.flatten() {
                let point = entry.path();
                let mounted = std::fs::symlink_metadata(&point)
                    .is_ok_and(|m| m.file_attributes() & REPARSE_POINT != 0);
                if mounted {
                    remotes.push(Remote {
                        source: entry.file_name(),
                        point,
                        fstype: fstype.clone(),
                        device: None,
                    });
                }
            }
        }
        (remotes, Vec::new())
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

    /// Files from anywhere are stat-ed a lane each: a remote's alone.
    #[cfg(target_os = "linux")]
    #[test]
    fn paths_are_grouped_by_their_lane_in_the_order_met() {
        let point = Path::new("/example/by lane/nas");
        pretend_remote(point);
        let paths = [
            PathBuf::from("/example/here/a.txt"),
            point.join("l'été.jpg"),
            PathBuf::from("/example/here/b.txt"),
            point.join("c.txt"),
        ];
        let lanes = by_lane(paths.iter(), |path| path.as_path());
        assert_eq!(lanes.len(), 2);
        assert_eq!((lanes[0].0, lanes[0].1.len()), (Lane::SHARED, 2));
        assert_eq!(lanes[1].0, lane(point));
        assert_eq!(lanes[1].1, [&paths[1], &paths[3]]);
    }

    /// Reads the user made from a server onto this machine prove it fast
    /// (10 MB/s and more) or slow again (under 5), this session, by its
    /// mount; a read too short, a copy within a server or onto another,
    /// and what lies between say nothing.
    #[cfg(target_os = "linux")]
    #[test]
    fn reads_onto_this_machine_prove_a_server_fast_or_slow() {
        let point = Path::new("/example/served by sshfs");
        pretend_remote(point);
        let file = point.join("a photo.jpg");
        let here = Path::new("/example/here");
        let remote = || of(point).unwrap();
        let mib = |n: u64| n << 20;
        let ms = Duration::from_millis;
        assert_eq!(proved_fast(&remote()), None, "nothing read yet");
        let changed = proved_changed();
        note_read(&file, here, mib(2), ms(10));
        note_read(&file, &point.join("elsewhere"), mib(64), ms(100));
        assert_eq!(proved_fast(&remote()), None, "too short, or within it");
        note_read(&file, here, mib(8), ms(400));
        assert_eq!(proved_fast(&remote()), Some(true), "21 MB/s");
        assert_eq!(proved_changed(), changed + 1);
        note_read(&file, here, mib(8), ms(1200));
        assert_eq!(proved_fast(&remote()), Some(true), "7 MB/s keeps it");
        assert_eq!(proved_changed(), changed + 1, "nothing changed");
        note_read(&file, here, mib(8), ms(4000));
        assert_eq!(proved_fast(&remote()), Some(false), "2 MB/s");
        assert_eq!(proved_changed(), changed + 2);
    }

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

    #[test]
    fn a_camera_moves_nothing_between_its_folders() {
        let remote = |point: &str, fstype: &str| Remote {
            point: PathBuf::from(point),
            fstype: fstype.to_owned(),
            source: OsString::new(),
            device: None,
        };
        let gvfs = "/run/user/1000/gvfs";
        let camera = remote(
            &format!("{gvfs}/gphoto2:host=%5Busb%3A001%2C004%5D"),
            "fuse.gvfsd-fuse",
        );
        assert!(
            !camera.moves_between_folders(),
            "GVfs's gPhoto2 renames within a folder"
        );
        let held = remote(
            "/run/user/1000/skwir/remote/kmtp mtp:%2F%2FPhone%2F",
            "fuse.skwir-kmtp",
        );
        assert!(!held.moves_between_folders(), "nor does KDE's kmtpd");
        assert!(network("fuse.skwir-kmtp", std::ffi::OsStr::new("kmtpd")));
        for (point, fstype) in [
            (format!("{gvfs}/mtp:host=Phone"), "fuse.gvfsd-fuse"),
            (format!("{gvfs}/sftp:host=nas"), "fuse.gvfsd-fuse"),
            ("/home/me/srv/gphoto2:x".to_owned(), "fuse.sshfs"),
            ("/mnt/nas".to_owned(), "nfs4"),
        ] {
            assert!(remote(&point, fstype).moves_between_folders(), "{point}");
        }
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
