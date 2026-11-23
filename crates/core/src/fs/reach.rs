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

use std::sync::atomic::{AtomicUsize, Ordering};

use parking_lot::{Mutex, RwLock};

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
    /// What the user calls it: `me@example.com`, `photos on nas`, said
    /// where it is shown.
    pub label: crate::words::Words,
    pub thumbnails: Thumbnails,
    /// What its program is told before a folder of it is read again, where
    /// it answers from what it read before (kio-fuse keeps a listing and a
    /// file's attributes 30 seconds): `App::read_again`.
    pub forget: Option<Forget>,
    /// Where its server keeps what is deleted there (“Google Drive's
    /// trash”), said where it is shown: a deletion's question says so
    /// rather than that it is gone for good (ADR-086). `None`: deleted for
    /// good, as on any network folder.
    pub trash: Option<crate::words::Words>,
    /// What its plugin asks of it when the window comes back
    /// (`App::refresh_remote`), shown or not: what other programs left
    /// waiting to be sent to its server (rclone's, ADR-102). Called in
    /// the remote's lane.
    pub looked_at: Option<Look>,
}

/// What a remote's plugin asks of it when the window comes back
/// (`Known::looked_at`). Called in the remote's lane: it may wait on the
/// remote's program.
#[derive(Clone)]
pub struct Look(pub Arc<dyn Fn() + Send + Sync>);

impl std::fmt::Debug for Look {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Look")
    }
}

impl PartialEq for Look {
    fn eq(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.0, &other.0)
    }
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

/// The volume holding `path`, by the path's text: a remote's mount point,
/// else the point of the deepest mount above it in the table in memory
/// (Linux, macOS), else its drive or share (Windows, which lists no
/// table); `None` where none is known. Never reads the disk nor a server:
/// what tells a move to another volume before it runs (ADR-135), as no
/// stat of a folder that may not answer can.
pub fn volume_of(path: &Path) -> Option<PathBuf> {
    if let Some(remote) = of(path) {
        return Some(remote.point.clone());
    }
    #[cfg(windows)]
    {
        use std::path::{Component, Prefix};
        let Some(Component::Prefix(prefix)) = path.components().next() else {
            return None;
        };
        match prefix.kind() {
            Prefix::Disk(letter) | Prefix::VerbatimDisk(letter) => Some(PathBuf::from(format!(
                "{}:\\",
                char::from(letter.to_ascii_uppercase())
            ))),
            Prefix::UNC(server, share) | Prefix::VerbatimUNC(server, share) => {
                let mut point = std::ffi::OsString::from(r"\\");
                point.push(server.to_ascii_lowercase());
                point.push(r"\");
                point.push(share.to_ascii_lowercase());
                Some(point.into())
            }
            _ => None,
        }
    }
    #[cfg(not(windows))]
    {
        table()
            .points
            .iter()
            .find(|mount| path.starts_with(&mount.point))
            .map(|mount| mount.point.clone())
    }
}

/// The folders a test says remotes are mounted at, and their types
/// ([`pretend_remote`]).
#[cfg(test)]
static PRETENDED_REMOTES: Mutex<Vec<(PathBuf, &str, Option<u64>)>> = Mutex::new(Vec::new());

/// The remotes a test says are mounted, as the table would list them:
/// every question asked of the table answers for them too ([`of`],
/// [`all`]), whatever this machine has mounted.
#[cfg(test)]
fn pretended() -> Vec<Arc<Remote>> {
    PRETENDED_REMOTES
        .lock()
        .iter()
        .map(|(point, fstype, device)| {
            Arc::new(Remote {
                point: point.clone(),
                fstype: (*fstype).to_owned(),
                source: "me@example.com:/".into(),
                device: *device,
            })
        })
        .collect()
}

/// Say, for this test's process (nextest runs each in one of its own),
/// that an sshfs remote is mounted at `point`: [`of`] says so of every
/// path below it.
#[cfg(all(test, target_os = "linux"))]
pub(crate) fn pretend_remote(point: &Path) {
    pretend_mount(point, "fuse.sshfs", None);
}

/// Say, for this test's process, that a remote of type `fstype` is
/// mounted at `point` on `device`.
#[cfg(all(test, target_os = "linux"))]
pub(crate) fn pretend_mount(point: &Path, fstype: &'static str, device: Option<u64>) {
    PRETENDED_REMOTES
        .lock()
        .push((point.to_path_buf(), fstype, device));
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
    pretend_mount(point, "fuse.gvfsd-fuse", None);
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

/// What a mount is known by while the table lists it: its point and its
/// device, as the table says them ([`key_of_mount`]), nothing asked of the
/// mount. What is known of it is let go of once a read of the table lists
/// it no more ([`forget_unmounted`]; at once where the table is followed,
/// [`follow`]), so a mount made again there is another.
type MountKey = (PathBuf, u64);

/// What `remote` is known by ([`MountKey`]): its point, and its device or,
/// a bridge's folder (which tells none), the bridge's.
fn key_of_mount(table: &Table, remote: &Remote) -> MountKey {
    let device = remote.device.or_else(|| {
        table
            .remotes
            .iter()
            .find(|r| remote.point.starts_with(&r.point))
            .and_then(|r| r.device)
    });
    (remote.point.clone(), device.unwrap_or(0))
}

/// What reads proved of each mount this session, fast or not, by what it
/// is known by ([`MountKey`]): a mount made again is unknown again. Kept in
/// memory alone, as the plugin's word on it is.
static PROVED: Mutex<Option<std::collections::HashMap<MountKey, bool>>> = Mutex::new(None);

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
    let key = key_of_mount(&table(), &remote);
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
    let key = key_of_mount(&table(), remote);
    PROVED.lock().as_ref()?.get(&key).copied()
}

/// How often what reads proved changed: a plugin that words a remote by it
/// compares it with the count it last saw.
pub fn proved_changed() -> u64 {
    PROVED_CHANGED.load(std::sync::atomic::Ordering::Relaxed)
}

/// The FUSE mounts that refused extended attributes, each by what it is
/// known by ([`MountKey`]: the point the refusal was asked under, a bridge's
/// folder such as GVfs's `afc:host=…`, and the device of the FUSE mount
/// that answered, a bridge's own), so a mount made again is asked again.
/// A FUSE program's refusal
/// holds for every file of its connection, and some still reach the
/// server first (GVfs's bridge looks each file up on an iPhone, a USB
/// round trip): asked once a mount, never again for each file a listing
/// reads (its tags, a lens of them). A kernel's share is not remembered:
/// its server may keep attributes in one folder and not another. Let go
/// of when the table lists that mount no more (`forget_unmounted`) or a
/// write there succeeds (`note_accepted_attributes`). In memory alone,
/// as what reads proved is; a refused mount or two, never a list of files.
static REFUSES_ATTRIBUTES: RwLock<Vec<MountKey>> = RwLock::new(Vec::new());

/// How many refusals [`REFUSES_ATTRIBUTES`] holds: a read where it holds
/// none asks nothing of the table and takes no lock.
static REFUSALS: AtomicUsize = AtomicUsize::new(0);

/// What a refusal of `path`'s extended attributes is remembered by: what
/// its FUSE remote is known by ([`MountKey`]); `None` on this machine's
/// volumes and a kernel's share.
fn refusing_mount(path: &Path) -> Option<MountKey> {
    let remote = of(path)?;
    if remote.by_kernel() {
        return None;
    }
    Some(key_of_mount(&table(), &remote))
}

/// Reading an extended attribute of `path` was refused as the volume
/// keeping none (`xattr::refused`): on a FUSE remote, that mount is not
/// asked again while it stays mounted ([`refuses_attributes`]). This
/// machine's volumes answer at once and a kernel's share may answer each
/// folder otherwise: both are asked each time.
pub fn note_refused_attributes(path: &Path) {
    let Some(key) = refusing_mount(path) else {
        return;
    };
    let mut refused = REFUSES_ATTRIBUTES.write();
    if !refused.contains(&key) {
        refused.push(key);
        REFUSALS.store(refused.len(), Ordering::Release);
    }
}

/// Writing an extended attribute of `path` succeeded: a refusal its mount
/// made is let go of, its attributes read again.
pub fn note_accepted_attributes(path: &Path) {
    if REFUSALS.load(Ordering::Acquire) == 0 {
        return;
    }
    let Some(key) = refusing_mount(path) else {
        return;
    };
    let mut refused = REFUSES_ATTRIBUTES.write();
    refused.retain(|kept| *kept != key);
    REFUSALS.store(refused.len(), Ordering::Release);
}

/// Whether `path`'s FUSE mount refused extended attributes and is still
/// the one mounted there ([`note_refused_attributes`]): a read there is
/// answered as it was. With none remembered, a load and nothing else.
pub fn refuses_attributes(path: &Path) -> bool {
    if REFUSALS.load(Ordering::Acquire) == 0 {
        return false;
    }
    // The deepest point remembered above `path`, asked of the table only
    // then: a read elsewhere takes no lock the table's readers share.
    let remembered = REFUSES_ATTRIBUTES
        .read()
        .iter()
        .filter(|(point, _)| path.starts_with(point))
        .max_by_key(|(point, _)| point.as_os_str().len())
        .cloned();
    remembered.is_some_and(|key| refusing_mount(path) == Some(key))
}

/// Let go of what is known of the mounts `table` lists no more (what
/// reads proved, the refusals): a mount made again at the same point, once
/// a read saw it gone, is another. From the table alone: no mount is
/// asked anything.
fn forget_unmounted(table: &Table) {
    let mounted = |(point, device): &MountKey| {
        let listed =
            |r: &Arc<Remote>| point.starts_with(&r.point) && r.device.unwrap_or(0) == *device;
        #[cfg(test)]
        if pretended().iter().any(&listed) {
            return true;
        }
        table.remotes.iter().any(listed)
    };
    if REFUSALS.load(Ordering::Acquire) > 0 {
        let mut refused = REFUSES_ATTRIBUTES.write();
        refused.retain(&mounted);
        REFUSALS.store(refused.len(), Ordering::Release);
    }
    if let Some(proved) = PROVED.lock().as_mut() {
        proved.retain(|known, _| mounted(known));
    }
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
    // A device that is a disk's is told without the table.
    #[cfg(all(target_os = "linux", not(test)))]
    if rustix::fs::major(device) != 0 {
        return 0;
    }
    number_in(&table(), device)
}

/// [`mount_of`] as `table` tells it.
fn number_in(table: &Table, device: u64) -> u64 {
    #[cfg(test)]
    if let Some(number) = pretended_number(device) {
        return number;
    }
    // FUSE mounts on no block device of its own: a device that has one
    // (a disk's, ntfs-3g's) is told without the table.
    #[cfg(target_os = "linux")]
    if rustix::fs::major(device) != 0 {
        return 0;
    }
    table.made_up.number(device)
}

/// The number a test says `device`'s mount has ([`pretend_made_up`]).
#[cfg(test)]
fn pretended_number(device: u64) -> Option<u64> {
    PRETENDED.with(|p| {
        p.borrow()
            .iter()
            .find(|(of, _)| *of == device)
            .map(|&(_, number)| number)
    })
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
    PRETENDED.with(|p| {
        let mut p = p.borrow_mut();
        p.retain(|(of, _)| *of != device);
        p.push((device, number));
    });
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
    // Followed from now on, where the table is followed ([`follow`]).
    #[cfg(windows)]
    imp::watch_new();
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

/// A mount the table lists ([`points`]): where it is mounted, what tells
/// it apart from another mount made at the same point since, and whether
/// it is a network's.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Mount {
    pub point: PathBuf,
    /// Linux's: its number in the table with what is mounted (its
    /// device, type, source and root), so a mount let go of and made again
    /// at once is another, its number being free again only once nothing
    /// holds the one before; macOS's: what is mounted (its source and
    /// type); the remotes of the folders programs mount in (Windows):
    /// theirs.
    pub id: u64,
    pub remote: bool,
    /// An automount point (autofs: systemd's automount units, Linux's
    /// `-hosts` maps, macOS's `/net`): what is mounted on it or below it is
    /// mounted as it is looked up, and let go of once nothing uses it a
    /// while, which is no volume going (ADR-149).
    pub on_demand: bool,
}

impl Mount {
    /// A mount at `point` of type `fstype`, told apart by `what`.
    fn of(point: PathBuf, fstype: &str, what: impl std::hash::Hash) -> Self {
        use std::hash::Hasher;
        let mut id = std::hash::DefaultHasher::new();
        what.hash(&mut id);
        Mount {
            point,
            id: id.finish(),
            remote: false,
            on_demand: fstype == "autofs",
        }
    }
}

/// Whether what holds `path` is mounted on demand: the deepest mount point
/// at or above it is an automount point, or holds a mount made on one, or
/// the point above it is one ([`Mount::on_demand`], [`on_demand_in`]). Its filesystem let go of after a while unused
/// is no volume going: looked up again, it is mounted again. From the
/// table in memory, never reading `path` (a lookup would mount it).
pub fn on_demand(path: &Path) -> bool {
    #[cfg(all(test, target_os = "linux"))]
    if PRETENDED_ON_DEMAND
        .lock()
        .iter()
        .any(|point| path.starts_with(point))
    {
        return true;
    }
    on_demand_in(&table().points, path)
}

/// [`on_demand`] in `points`, deepest first: the point holding `path`
/// holds an automount point (a direct one's: systemd stacks the
/// filesystem on its autofs), or the next point above it holds one alone
/// (an indirect map's: `/net/host` below `/net`, `/misc/cd` below
/// `/misc`). So a table in memory that still lists the mount just let go
/// of answers as one read again after it would, whose deepest point
/// holding `path` is the map's: never a share expired taken for a folder
/// deleted. Below a direct point, what holds something else too, a mount
/// is someone's own (a stick mounted in a folder systemd automounts), not
/// the automounter's.
fn on_demand_in(points: &[Mount], path: &Path) -> bool {
    let holding: Vec<&Mount> = points
        .iter()
        .filter(|mount| path.starts_with(&mount.point))
        .collect();
    let Some(own) = holding.first().map(|mount| &mount.point) else {
        return false;
    };
    let (at_own, above): (Vec<&Mount>, Vec<&Mount>) =
        holding.iter().partition(|mount| mount.point == *own);
    if at_own.iter().any(|mount| mount.on_demand) {
        return true;
    }
    let Some(next) = above.first().map(|mount| &mount.point) else {
        return false;
    };
    above
        .iter()
        .take_while(|mount| mount.point == *next)
        .all(|mount| mount.on_demand)
}

/// The automount points a test says hold the paths below them
/// ([`pretend_on_demand`]).
#[cfg(all(test, target_os = "linux"))]
static PRETENDED_ON_DEMAND: Mutex<Vec<PathBuf>> = Mutex::new(Vec::new());

/// Say, for this test's process, that what is below `point` is mounted on
/// demand ([`on_demand`]).
#[cfg(all(test, target_os = "linux"))]
pub(crate) fn pretend_on_demand(point: &Path) {
    PRETENDED_ON_DEMAND.lock().push(point.to_path_buf());
}

/// Every mount the table lists now: what tells the mounts made and let
/// go of since a read before (`App::volumes_changed`, ADR-149), by what
/// each is, not by how many a point holds, so one let go of and made
/// again within a change is both. Where the system lists no table
/// (Windows), the remotes of the folders programs mount in. From memory,
/// read again when stale.
pub fn points() -> Vec<Mount> {
    let table = table();
    let listed: crate::hashing::Set<&Path> =
        table.points.iter().map(|m| m.point.as_path()).collect();
    let remotes = table
        .remotes
        .iter()
        .filter(|r| !listed.contains(r.point.as_path()))
        .map(|r| Mount {
            remote: true,
            ..Mount::of(r.point.clone(), &r.fstype, (&r.fstype, &r.source))
        });
    table.points.iter().cloned().chain(remotes).collect()
}

/// Forget the table read: a mount was made or let go of here.
pub fn changed() {
    *TABLE.lock() = None;
}

/// How long the table is let settle once the system said it changed,
/// before its followers are told: a burst (a program mounting several, a
/// container starting) makes one call.
const SETTLE: Duration = Duration::from_millis(250);

/// What follows the table ([`follow`]): each one's lifetime and call.
type Follower = (crate::cancel::CancelToken, Arc<dyn Fn() + Send + Sync>);

static FOLLOWERS: Mutex<Vec<Follower>> = Mutex::new(Vec::new());

/// Whether the system tells the table's changes, once first asked
/// ([`follow`]).
static FOLLOWED: std::sync::OnceLock<bool> = std::sync::OnceLock::new();

/// Follow the table: `told` runs once a change of it settles, a mount
/// made or let go of by any program, until `lifetime` ends; the table is
/// read again by then ([`changed`]). On the table's own thread, one for
/// every follower: `told` starts work and never waits for it. Whether
/// this system tells: Linux (the kernel marks `/proc/self/mountinfo`
/// changed), macOS (kqueue's file system events), Windows (the folders
/// programs mount in, [`mounts_in`]); elsewhere nothing is told, and what
/// shows mounts asks again when a window comes to the front.
pub fn follow(
    lifetime: &crate::cancel::CancelToken,
    told: impl Fn() + Send + Sync + 'static,
) -> bool {
    let followed = *FOLLOWED.get_or_init(imp::follow);
    if followed {
        // Those whose lifetime ended let go of here too: a plugin turned
        // off and on again, the table unchanged meanwhile.
        let mut followers = FOLLOWERS.lock();
        followers.retain(|(lifetime, _)| !lifetime.is_cancelled());
        followers.push((lifetime.clone(), Arc::new(told)));
    }
    followed
}

/// The system said the table changed: it is read again, what is known of
/// a mount it lists no more let go of ([`forget_unmounted`]), and every
/// follower whose lifetime goes on told.
fn told() {
    changed();
    drop(table());
    let followers: Vec<Arc<dyn Fn() + Send + Sync>> = {
        let mut followers = FOLLOWERS.lock();
        followers.retain(|(lifetime, _)| !lifetime.is_cancelled());
        followers.iter().map(|(_, told)| Arc::clone(told)).collect()
    };
    for told in followers {
        told();
    }
}

/// What a wait on the system's word that the table changed ended with.
#[cfg(any(target_os = "linux", target_os = "macos"))]
#[derive(Clone, Copy, PartialEq, Eq)]
enum Heard {
    Changed,
    /// Nothing said within the wait.
    Quiet,
    /// The system refused the wait: nothing is followed any more.
    Failed,
}

/// Wait on `heard` for as long as it takes, then for the rest of a burst
/// (at most `watch::SETTLED_WITHIN`), and tell the followers ([`told`]):
/// on a thread of its own, until the system refuses a wait. Whether the
/// thread started.
#[cfg(any(target_os = "linux", target_os = "macos"))]
fn follow_with(heard: impl Fn(Option<Duration>) -> Heard + Send + 'static) -> bool {
    std::thread::Builder::new()
        .name("skwir-mounts".into())
        .spawn(move || {
            loop {
                match heard(None) {
                    Heard::Changed => {}
                    Heard::Quiet => continue,
                    Heard::Failed => return,
                }
                let mut ended = false;
                // Told at least every `SETTLED_WITHIN` while a burst goes on.
                let ceiling = Instant::now() + crate::fs::watch::SETTLED_WITHIN;
                while let Some(left) = ceiling.checked_duration_since(Instant::now()) {
                    match heard(Some(SETTLE.min(left))) {
                        Heard::Changed => {}
                        Heard::Quiet => break,
                        Heard::Failed => {
                            ended = true;
                            break;
                        }
                    }
                }
                told();
                crate::scheduler::release_now();
                if ended {
                    return;
                }
            }
        })
        .is_ok()
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
    drop(held);
    forget_unmounted(&table);
    table
}

static TABLE: Mutex<Option<(Instant, Arc<Table>)>> = Mutex::new(None);

/// The FUSE mounts of the table, whose programs make their files' inode
/// numbers up, each with its mount's number ([`mount_of`]), worked out
/// the first time its own device is asked: never by a read of the table,
/// nor for another mount than the one asked. Asking a FUSE mount its
/// unique number holds the mount for that moment, and every process
/// following the table reads it as a mount comes or goes ([`follow`]): a
/// program letting go of its mount just then was refused, the mount busy.
#[derive(Default)]
struct MadeUp {
    #[cfg(target_os = "linux")]
    mounts: Vec<imp::Numbered>,
}

impl MadeUp {
    /// The number of the mount `device`'s files are on, 0 where no FUSE
    /// mount of the table is on it: asks that mount alone, once.
    // Only Linux's table holds the mounts it numbers.
    #[cfg_attr(not(target_os = "linux"), allow(clippy::unused_self))]
    fn number(&self, device: u64) -> u64 {
        #[cfg(target_os = "linux")]
        if let Some(mount) = self.mounts.iter().find(|m| m.device == device) {
            return mount.number(imp::unique);
        }
        let _ = device;
        0
    }
}

/// The network mounts, deepest point first, the devices whose inode
/// numbers are made up, each with its mount's number ([`mount_of`]), and
/// every mount's point, deepest first ([`volume_of`]), where the system
/// lists them (not Windows).
#[derive(Default)]
struct Table {
    remotes: Vec<Arc<Remote>>,
    made_up: MadeUp,
    // Windows lists no table: its volumes are told by a path's drive or
    // share (`volume_of`), and its list stays empty, never read.
    #[cfg_attr(windows, allow(dead_code))]
    points: Vec<Mount>,
}

impl Table {
    fn read() -> Table {
        let (remotes, made_up, mut points) = imp::read();
        let mut remotes: Vec<Arc<Remote>> = remotes.into_iter().map(Arc::new).collect();
        remotes.sort_by_key(|r| std::cmp::Reverse(r.point.as_os_str().len()));
        let remote: crate::hashing::Set<&Path> =
            remotes.iter().map(|r| r.point.as_path()).collect();
        for mount in &mut points {
            mount.remote = remote.contains(mount.point.as_path());
        }
        points.sort_by_key(|mount| std::cmp::Reverse(mount.point.as_os_str().len()));
        Table {
            remotes,
            made_up,
            points,
        }
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

/// Whether `remote` is a bridge ([`BRIDGES`]): one mount of the table
/// holding many, listed by [`bridged`].
pub fn is_bridge(remote: &Remote) -> bool {
    bridge_depth(remote).is_some()
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

    /// Follow the table ([`super::follow`]): the kernel marks
    /// `/proc/self/mountinfo` held open (`POLLPRI`) once a mount of this
    /// namespace was made or let go of since it was last asked, and wakes
    /// nothing otherwise.
    pub(super) fn follow() -> bool {
        use super::Heard;
        use rustix::event::{PollFd, PollFlags, Timespec};
        let Ok(table) = std::fs::File::open("/proc/self/mountinfo") else {
            return false;
        };
        super::follow_with(move |within| {
            let timeout = within.and_then(|wait| Timespec::try_from(wait).ok());
            let mut asked = [PollFd::new(&table, PollFlags::PRI)];
            match rustix::event::poll(&mut asked, timeout.as_ref()) {
                Ok(0) | Err(rustix::io::Errno::INTR) => Heard::Quiet,
                Ok(_) => {
                    let said = asked[0].revents();
                    if said.intersects(PollFlags::PRI | PollFlags::ERR) {
                        Heard::Changed
                    } else if said.is_empty() {
                        Heard::Quiet
                    } else {
                        Heard::Failed
                    }
                }
                Err(_) => Heard::Failed,
            }
        })
    }

    /// The table: its network mounts, its FUSE mounts for the numbers made
    /// up (each worked out when its device is asked, `MadeUp`), its points.
    /// Reads procfs alone.
    pub(super) fn read() -> (Vec<Remote>, super::MadeUp, Vec<super::Mount>) {
        let Ok(text) = std::fs::read("/proc/self/mountinfo") else {
            return (Vec::new(), super::MadeUp::default(), Vec::new());
        };
        let mounts = crate::fs::volume::mountinfo(&text);
        let points = mounts
            .iter()
            .map(|m| {
                super::Mount::of(
                    m.point.clone(),
                    &m.fstype,
                    (m.id, m.device, &m.fstype, &m.source, &m.root),
                )
            })
            .collect();
        (
            remotes(&mounts),
            super::MadeUp {
                mounts: made_up(&mounts),
            },
            points,
        )
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

    /// A FUSE mount of the table by its device, its number worked out the
    /// first time it is asked ([`Numbered::number`]).
    pub(super) struct Numbered {
        pub(super) device: u64,
        point: std::path::PathBuf,
        /// Whether its point may be asked: not below another FUSE mount
        /// or a network's, whose program or server would look it up.
        asked: bool,
        /// Its number in the table.
        id: Option<u64>,
        number: std::sync::OnceLock<u64>,
    }

    impl Numbered {
        /// Its number: `unique`'s for its point, else the table's; 0 for
        /// neither. Asked once.
        pub(super) fn number(&self, unique: impl FnOnce(&Path) -> Option<u64>) -> u64 {
            *self.number.get_or_init(|| {
                self.asked
                    .then(|| unique(&self.point))
                    .flatten()
                    .or(self.id)
                    .map_or(0, |number| number.max(1))
            })
        }

        /// Whether its number was asked already: what a test asks.
        #[cfg(test)]
        pub(super) fn asked_yet(&self) -> bool {
            self.number.get().is_some()
        }
    }

    /// The FUSE mounts of `mounts`, nothing asked of them yet.
    /// `fuseblk` (ntfs-3g) keeps its volume's numbers. A point below
    /// another FUSE mount or a network's is never asked: its path would be
    /// looked up through that mount's program or server, which may not
    /// answer.
    pub(super) fn made_up(mounts: &[Mounted]) -> Vec<Numbered> {
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
                Some(Numbered {
                    device: m.device?,
                    point: m.point.clone(),
                    asked: !below,
                    id: m.id,
                    number: std::sync::OnceLock::new(),
                })
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

    /// `VQ_MOUNT` and `VQ_UNMOUNT` (`sys/mount.h`), which the libc crate
    /// lacks: what a file system event says of a mount made or let go of.
    const VQ_MOUNT: u32 = 0x0008;
    const VQ_UNMOUNT: u32 = 0x0010;

    /// Follow the table ([`super::follow`]): kqueue's file system filter
    /// (`EVFILT_FS`) says a mount was made or let go of, and wakes nothing
    /// otherwise.
    pub(super) fn follow() -> bool {
        use super::Heard;
        // SAFETY: a new queue, the thread's below alone, or closed here.
        #[allow(unsafe_code)]
        let queue = unsafe { libc::kqueue() };
        if queue < 0 {
            return false;
        }
        let asked = libc::kevent {
            ident: 0,
            filter: libc::EVFILT_FS,
            flags: libc::EV_ADD | libc::EV_CLEAR,
            fflags: 0,
            data: 0,
            udata: std::ptr::null_mut(),
        };
        // SAFETY: one change read from `asked`; no event asked back.
        #[allow(unsafe_code)]
        let added = unsafe {
            libc::kevent(
                queue,
                &raw const asked,
                1,
                std::ptr::null_mut(),
                0,
                std::ptr::null(),
            )
        };
        let close = || {
            // SAFETY: the queue made above, closed once.
            #[allow(unsafe_code)]
            unsafe {
                libc::close(queue);
            }
        };
        if added < 0 {
            close();
            return false;
        }
        let followed = super::follow_with(move |within| {
            let timeout = within.map(|wait| libc::timespec {
                tv_sec: libc::time_t::try_from(wait.as_secs()).unwrap_or(libc::time_t::MAX),
                tv_nsec: libc::c_long::from(wait.subsec_nanos()),
            });
            // SAFETY: plain data, all zeros a valid value; filled by the call.
            #[allow(unsafe_code)]
            let mut said: libc::kevent = unsafe { std::mem::zeroed() };
            // SAFETY: room for one event in `said`; the time, where given,
            // lives across the call.
            #[allow(unsafe_code)]
            let got = unsafe {
                libc::kevent(
                    queue,
                    std::ptr::null(),
                    0,
                    &raw mut said,
                    1,
                    timeout
                        .as_ref()
                        .map_or(std::ptr::null(), std::ptr::from_ref),
                )
            };
            match got {
                0 => Heard::Quiet,
                got if got < 0 => {
                    if std::io::Error::last_os_error().kind() == std::io::ErrorKind::Interrupted {
                        Heard::Quiet
                    } else {
                        Heard::Failed
                    }
                }
                _ if said.fflags & (VQ_MOUNT | VQ_UNMOUNT) != 0 => Heard::Changed,
                // Another of its words (a volume short of room).
                _ => Heard::Quiet,
            }
        });
        if !followed {
            close();
        }
        followed
    }

    /// macOS's mounts that are not local (`MNT_LOCAL` unset), and every
    /// mount's point; no device is told whose numbers are made up.
    pub(super) fn read() -> (Vec<Remote>, super::MadeUp, Vec<super::Mount>) {
        let (remotes, points) = mounts();
        (remotes, super::MadeUp::default(), points)
    }

    /// macOS's mounts that are not local (`MNT_LOCAL` unset), and every
    /// mount's point, read by `getfsstat` without waiting on any
    /// (`MNT_NOWAIT`).
    fn mounts() -> (Vec<Remote>, Vec<super::Mount>) {
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
            return (Vec::new(), Vec::new());
        };
        let mut table: Vec<libc::statfs> = Vec::with_capacity(count + 8);
        let Ok(bytes) = libc::c_int::try_from(table.capacity() * size_of::<libc::statfs>()) else {
            return (Vec::new(), Vec::new());
        };
        // SAFETY: the buffer has room for `bytes` bytes; the call writes
        // whole records within them and says how many.
        #[allow(unsafe_code)]
        let filled = unsafe { libc::getfsstat(table.as_mut_ptr(), bytes, libc::MNT_NOWAIT) };
        let Ok(filled) = usize::try_from(filled) else {
            return (Vec::new(), Vec::new());
        };
        // SAFETY: the call initialized the first `filled` records.
        #[allow(unsafe_code)]
        unsafe {
            table.set_len(filled.min(table.capacity()));
        }
        let remotes = table
            .iter()
            .filter(|fs| fs.f_flags & (libc::MNT_LOCAL as u32) == 0)
            .map(|fs| Remote {
                point: std::ffi::OsStr::from_bytes(&name(&fs.f_mntonname)).into(),
                fstype: String::from_utf8_lossy(&name(&fs.f_fstypename)).into_owned(),
                source: std::ffi::OsStr::from_bytes(&name(&fs.f_mntfromname)).into(),
                device: None,
            })
            .collect();
        let points = table
            .iter()
            .map(|fs| {
                let fstype = String::from_utf8_lossy(&name(&fs.f_fstypename)).into_owned();
                super::Mount::of(
                    std::ffi::OsStr::from_bytes(&name(&fs.f_mntonname)).into(),
                    &fstype,
                    (name(&fs.f_mntfromname), &fstype),
                )
            })
            .collect();
        (remotes, points)
    }
}

#[cfg(windows)]
mod imp {
    use std::path::{Component, Path, Prefix};

    use super::Remote;

    /// What follows the folders programs mount in, once the table is
    /// followed, and those it follows. Taken before `MOUNT_FOLDERS`.
    static WATCHER: parking_lot::Mutex<Option<(crate::fs::Settled, Vec<std::path::PathBuf>)>> =
        parking_lot::Mutex::new(None);

    /// Follow the table ([`super::follow`]): the table is the folders
    /// programs mount in ([`super::mounts_in`]), a mount made there or let
    /// go of a folder of theirs come or gone.
    pub(super) fn follow() -> bool {
        let Some(watcher) = crate::fs::Settled::start(
            "skwir-mounts",
            super::SETTLE,
            |_| true,
            || {
                super::told();
                true
            },
        ) else {
            return false;
        };
        *WATCHER.lock() = Some((watcher, Vec::new()));
        watch_new();
        true
    }

    /// Follow the folders programs mount in that are not followed yet,
    /// where the table is followed: after one is said, and once followed,
    /// whichever comes last.
    pub(super) fn watch_new() {
        let mut held = WATCHER.lock();
        let Some((watcher, followed)) = held.as_mut() else {
            return;
        };
        for (folder, _) in super::MOUNT_FOLDERS.lock().iter() {
            if !followed.contains(folder) && watcher.watch(folder) {
                followed.push(folder.clone());
            }
        }
    }

    /// Windows lists no table: a path is told by its own form
    /// ([`share_of`]), and a program's mounts in a folder it said by their
    /// being mount points there ([`super::mounts_in`]).
    pub(super) fn read() -> (Vec<Remote>, super::MadeUp, Vec<super::Mount>) {
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
        (remotes, super::MadeUp::default(), Vec::new())
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
    /// Nothing tells the table's changes here.
    pub(super) fn follow() -> bool {
        false
    }

    pub(super) fn read() -> (Vec<super::Remote>, super::MadeUp, Vec<super::Mount>) {
        (Vec::new(), super::MadeUp::default(), Vec::new())
    }
}

#[cfg(test)]
mod tests {
    /// A mount made and let go of by another program is told, once each,
    /// and the table read again by then. Run in a mount namespace of the
    /// test's own (`the_table_is_followed`), where it mounts as root: it
    /// does nothing anywhere else.
    ///
    /// Its namespace holds a copy of every mount there was as it was made,
    /// and another namespace removing the folder one of them is on (a test
    /// beside it taking its fixture away once it let go of its FUSE mount)
    /// detaches the copy here too: a change of this table, told as any is.
    /// So each call is held to the table having changed since the call
    /// before, as the follower read it then (`points`): a call for a table
    /// that did not change fails, a burst being one call, whatever else
    /// another namespace makes this table tell.
    #[cfg(target_os = "linux")]
    #[test]
    fn a_mount_made_or_let_go_of_is_told() {
        use std::process::Command;
        use std::time::{Duration, Instant};
        if !crate::testing::in_namespace() {
            return;
        }
        let (tx, rx) = crossbeam_channel::unbounded();
        let lifetime = crate::cancel::CancelToken::new();
        assert!(super::follow(&lifetime, move || {
            let _ = tx.send(());
        }));
        let point = tempfile::tempdir().unwrap();
        let point = point.path().join("a mount's point");
        std::fs::create_dir(&point).unwrap();
        let mounted_here = || super::volume_of(&point).as_deref() == Some(point.as_path());
        // Read before: kept until told.
        assert!(!mounted_here());
        let mut last = super::points();
        // A call, the table read again for it changed since the call before.
        let mut told = |within: Duration| -> bool {
            if rx.recv_timeout(within).is_err() {
                return false;
            }
            let now = super::points();
            assert_ne!(now, last, "told with nothing changed");
            last = now;
            true
        };
        let mounted = Command::new("mount")
            .args(["-t", "tmpfs", "skwir-test"])
            .arg(&point)
            .status()
            .unwrap();
        assert!(mounted.success());
        // Told, as many times as other namespaces changed the table too,
        // until the table read again for a call holds it.
        let deadline = Instant::now() + Duration::from_secs(10);
        let left = |deadline: Instant| deadline.saturating_duration_since(Instant::now());
        assert!(told(left(deadline)), "the mount was told");
        while !mounted_here() {
            assert!(told(left(deadline)), "the mount was told");
        }
        let let_go = Command::new("umount").arg(&point).status().unwrap();
        assert!(let_go.success());
        let deadline = Instant::now() + Duration::from_secs(10);
        assert!(told(left(deadline)), "letting go of it was told");
        while mounted_here() {
            assert!(told(left(deadline)), "letting go of it was told");
        }
        // A burst is one call: what is told now is another change, never
        // this one again.
        let quiet = Instant::now() + Duration::from_secs(10);
        while told(Duration::from_millis(600)) && Instant::now() < quiet {}
    }

    /// A follower whose lifetime ended is let go of as another follows,
    /// the table unchanged meanwhile: a plugin turned off and on again
    /// leaves nothing behind it.
    #[cfg(target_os = "linux")]
    #[test]
    fn a_follower_whose_lifetime_ended_is_let_go_of() {
        let gone = crate::cancel::CancelToken::new();
        assert!(super::follow(&gone, || {}));
        gone.cancel();
        let kept = crate::cancel::CancelToken::new();
        assert!(super::follow(&kept, || {}));
        assert_eq!(super::FOLLOWERS.lock().len(), 1);
    }

    /// Reading the table asks no mount anything: the number a FUSE mount
    /// is asked for is worked out once its own device is asked
    /// ([`mount_of`]), and what is known of a mount (what reads proved, a
    /// refusal) is known by what the table holds, its point and device,
    /// and let go of from the table alone. Every process following the
    /// table reads it as a mount comes or goes; a mount asked then is busy
    /// for the program letting go of it.
    #[cfg(target_os = "linux")]
    #[test]
    fn reading_the_table_asks_no_mount() {
        let read = Table::read();
        assert!(
            read.made_up.mounts.iter().all(|m| !m.asked_yet()),
            "nothing asked yet"
        );
        let text =
            b"36 25 0:88 / /run/user/1000/gvfs rw shared:922 - fuse.gvfsd-fuse gvfsd-fuse rw\n\
37 25 0:98 / /home/me/srv rw shared:9 - fuse.sshfs me@host:/ rw\n\
39 25 0:91 / /mnt/nas rw shared:11 - nfs4 nas:/export rw\n";
        let of_text = |text: &[u8]| {
            let mounts = crate::fs::volume::mountinfo(text);
            let mut remotes: Vec<Arc<Remote>> =
                imp::remotes(&mounts).into_iter().map(Arc::new).collect();
            remotes.sort_by_key(|r| std::cmp::Reverse(r.point.as_os_str().len()));
            Table {
                remotes,
                made_up: MadeUp {
                    mounts: imp::made_up(&mounts),
                },
                points: Vec::new(),
            }
        };
        let table = of_text(text);
        let dev = |minor| rustix::fs::makedev(0, minor);
        let sshfs = table.of(Path::new("/home/me/srv/l'été.jpg")).unwrap();
        let phone = table
            .of(Path::new("/run/user/1000/gvfs/mtp:host=Phone/DCIM"))
            .unwrap();
        let nfs = table.of(Path::new("/mnt/nas/a")).unwrap();
        let (sshfs, phone, nfs): (MountKey, MountKey, MountKey) = (
            key_of_mount(&table, &sshfs),
            key_of_mount(&table, &phone),
            key_of_mount(&table, &nfs),
        );
        assert_eq!(sshfs, (PathBuf::from("/home/me/srv"), dev(98)));
        assert_eq!(
            phone,
            (PathBuf::from("/run/user/1000/gvfs/mtp:host=Phone"), dev(88)),
            "a bridge's folder by the bridge's device"
        );
        REFUSES_ATTRIBUTES
            .write()
            .extend([sshfs.clone(), phone.clone()]);
        REFUSALS.store(2, Ordering::Release);
        PROVED
            .lock()
            .get_or_insert_with(Default::default)
            .extend([(nfs.clone(), true), (sshfs.clone(), false)]);
        forget_unmounted(&table);
        assert_eq!(REFUSALS.load(Ordering::Acquire), 2, "all still mounted");
        assert_eq!(
            PROVED.lock().as_ref().map(std::collections::HashMap::len),
            Some(2)
        );
        // sshfs let go of, and mounted again at its point on another device.
        let since = of_text(
            b"36 25 0:88 / /run/user/1000/gvfs rw shared:922 - fuse.gvfsd-fuse gvfsd-fuse rw\n\
40 25 0:99 / /home/me/srv rw shared:9 - fuse.sshfs me@host:/ rw\n",
        );
        forget_unmounted(&since);
        assert_eq!(*REFUSES_ATTRIBUTES.read(), [phone]);
        assert!(
            PROVED
                .lock()
                .as_ref()
                .is_some_and(std::collections::HashMap::is_empty)
        );
        assert!(
            table
                .made_up
                .mounts
                .iter()
                .chain(&since.made_up.mounts)
                .all(|m| !m.asked_yet()),
            "no mount asked"
        );
        assert_eq!(number_in(&since, dev(98)), 0, "no mount on it now");
        assert!(since.made_up.mounts.iter().all(|m| !m.asked_yet()));
        let _ = number_in(&since, dev(99));
        let asked: Vec<bool> = since
            .made_up
            .mounts
            .iter()
            .map(imp::Numbered::asked_yet)
            .collect();
        assert_eq!(asked, [false, true], "the mount asked alone");
    }

    /// [`a_mount_made_or_let_go_of_is_told`], run as root in a user and
    /// mount namespace of its own: the mounts it makes are seen there
    /// alone. Skipped where user namespaces are not allowed.
    #[cfg(target_os = "linux")]
    #[test]
    fn the_table_is_followed() {
        if !crate::testing::in_namespace() {
            crate::testing::as_root_in_a_namespace(
                "fs::reach::tests::a_mount_made_or_let_go_of_is_told",
            );
        }
    }

    /// What is mounted on demand is told by the table alone, as it stands
    /// in memory: a share mounted on an automount point, one an indirect
    /// map mounted below its point (listed still, a moment after the
    /// automounter let it go, or not), never a drive mounted below a folder
    /// that is no automount point, nor below a direct automount point,
    /// whose own filesystem is mounted on it.
    #[test]
    fn what_is_mounted_on_demand_is_told_by_the_table() {
        let table = |mounts: &[(&str, &str)]| {
            let mut mounts: Vec<Mount> = mounts
                .iter()
                .map(|(point, fstype)| Mount::of(PathBuf::from(point), fstype, (point, fstype)))
                .collect();
            mounts.sort_by_key(|mount| std::cmp::Reverse(mount.point.as_os_str().len()));
            mounts
        };
        let mounted = table(&[
            ("/", "ext4"),
            ("/misc", "autofs"),
            ("/misc/cd", "iso9660"),
            ("/net", "autofs"),
            ("/net/host", "nfs4"),
            ("/mnt/nas", "autofs"),
            ("/mnt/nas", "nfs4"),
            ("/media", "tmpfs"),
            ("/media/u/USB", "vfat"),
            ("/net/host/deeper", "nfs4"),
            ("/data", "autofs"),
            ("/data", "ext4"),
            ("/data/usb", "vfat"),
        ]);
        for path in [
            "/misc/cd/it's a track.flac",
            "/misc/cd",
            "/net/host/share, été",
            "/mnt/nas/photos",
            "/mnt/nas",
            "/data/it's there",
        ] {
            assert!(on_demand_in(&mounted, Path::new(path)), "{path}");
        }
        for path in [
            "/home/u/it's mine",
            "/media/u/USB/photos",
            "/net/host/deeper/x",
            "/miscellaneous",
            // Below a direct automount point, what is mounted there is
            // someone's own: a stick, not the automounter's.
            "/data/usb/photos",
        ] {
            assert!(!on_demand_in(&mounted, Path::new(path)), "{path}");
        }
        // Read again once the automounter let them go: the same answers.
        let expired = table(&[
            ("/", "ext4"),
            ("/misc", "autofs"),
            ("/net", "autofs"),
            ("/mnt/nas", "autofs"),
        ]);
        for path in [
            "/misc/cd/it's a track.flac",
            "/net/host/share, été",
            "/mnt/nas/photos",
        ] {
            assert!(on_demand_in(&expired, Path::new(path)), "{path}");
        }
        assert!(!on_demand_in(&[], Path::new("/net/host")));
    }

    /// A path's volume is the deepest mount above it, told by its text
    /// alone from the table in memory: nothing is read of the path.
    #[cfg(target_os = "linux")]
    #[test]
    fn a_path_s_volume_is_the_deepest_mount_above_it() {
        use std::path::Path;
        assert_eq!(
            super::volume_of(Path::new("/proc/self/status")).as_deref(),
            Some(Path::new("/proc"))
        );
        assert_eq!(
            super::volume_of(Path::new("/")).as_deref(),
            Some(Path::new("/"))
        );
        // Not there: told by its text all the same.
        assert_eq!(
            super::volume_of(Path::new("/proc/not there, it's made up")).as_deref(),
            Some(Path::new("/proc"))
        );
    }

    use super::*;

    /// A FUSE remote that refused extended attributes is not asked again
    /// while mounted, its files answered as it answered (procfs stands for
    /// a server that keeps none: it refuses `user.*`); a local volume that
    /// refuses is asked each time.
    #[cfg(target_os = "linux")]
    #[test]
    fn a_remote_that_refused_attributes_is_not_asked_again() {
        let point = Path::new("/proc/self");
        let file = point.join("status");
        assert!(!refuses_attributes(&file));
        let local = crate::fs::xattr::get(&file, "user.xdg.tags").unwrap_err();
        assert!(crate::fs::xattr::refused(&local), "{local}");
        assert!(!refuses_attributes(&file), "a local volume is asked again");
        pretend_remote(point);
        let first = crate::fs::xattr::get(&file, "user.xdg.tags").unwrap_err();
        assert!(crate::fs::xattr::refused(&first));
        assert!(refuses_attributes(&file));
        assert!(
            refuses_attributes(&point.join("stat")),
            "its other files too"
        );
        for again in [
            crate::fs::xattr::get_deref(point.join("stat"), "user.xdg.tags").map(drop),
            crate::fs::xattr::list(point.join("cmdline")).map(drop),
        ] {
            let error = again.unwrap_err();
            assert!(crate::fs::xattr::refused(&error), "{error}");
            assert!(error.to_string().contains("keeps no extended attributes"));
        }
        assert!(!refuses_attributes(Path::new("/example/elsewhere/a.txt")));
    }

    /// A kernel's share is asked each time: its server may keep attributes
    /// in one folder and not another.
    #[cfg(target_os = "linux")]
    #[test]
    fn a_share_the_kernel_mounts_is_asked_each_time() {
        let point = Path::new("/proc/self");
        pretend_mount(point, "nfs4", None);
        let file = point.join("status");
        let refused = crate::fs::xattr::get(&file, "user.xdg.tags").unwrap_err();
        assert!(crate::fs::xattr::refused(&refused), "{refused}");
        assert!(!refuses_attributes(&file));
        assert_eq!(REFUSALS.load(Ordering::Acquire), 0);
    }

    /// A FUSE mount made again is asked again: its refusal is kept by what
    /// the table knows it by, its point and device, and let go of once the
    /// table lists it no more.
    #[cfg(target_os = "linux")]
    #[test]
    fn a_mount_made_again_is_asked_again() {
        let point = Path::new("/proc/self");
        let file = point.join("status");
        // A FUSE mount's device is no disk's: major 0.
        let device = rustix::fs::makedev(0, 77);
        pretend_mount(point, "fuse.sshfs", Some(device));
        assert!(crate::fs::xattr::get(&file, "user.xdg.tags").is_err());
        assert!(refuses_attributes(&file));
        // Let go of and mounted again at the same point: another device.
        PRETENDED_REMOTES.lock().clear();
        pretend_mount(point, "fuse.sshfs", Some(rustix::fs::makedev(0, 78)));
        assert!(!refuses_attributes(&file), "a mount made again is asked");
        changed();
        assert!(of(Path::new("/example/elsewhere")).is_none());
        assert_eq!(REFUSALS.load(Ordering::Acquire), 0, "the old one let go of");
    }

    /// A write that succeeds lets go of the refusal its mount made: its
    /// attributes are read again.
    #[cfg(target_os = "linux")]
    #[test]
    fn a_write_that_succeeds_lets_go_of_a_refusal() {
        let base = dirs::home_dir().unwrap().join(".cache/skwir-tests");
        std::fs::create_dir_all(&base).unwrap();
        let tmp = tempfile::tempdir_in(&base).unwrap();
        let file = tmp.path().join("it's a note.txt");
        std::fs::write(&file, "x").unwrap();
        pretend_remote(tmp.path());
        note_refused_attributes(&file);
        assert!(refuses_attributes(&file));
        assert!(crate::fs::xattr::get(&file, "user.xdg.comment").is_err());
        crate::fs::xattr::set(&file, "user.xdg.comment", b"kept").unwrap();
        assert!(!refuses_attributes(&file));
        assert_eq!(
            crate::fs::xattr::get(&file, "user.xdg.comment").unwrap(),
            Some(b"kept".to_vec())
        );
    }

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
            made_up: MadeUp::default(),
            points: Vec::new(),
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
            made_up: MadeUp::default(),
            points: Vec::new(),
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
        let made_up: Vec<(u64, u64)> = imp::made_up(&mounts)
            .iter()
            .map(|m| {
                let number = m.number(|point| {
                    asked.lock().push(point.to_path_buf());
                    (point != Path::new("/home/me/srv")).then_some(1 << 40)
                });
                (m.device, number)
            })
            .collect();
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
