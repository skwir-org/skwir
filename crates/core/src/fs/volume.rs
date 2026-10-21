//! What kind of drive a path is on, as the kernel describes it in sysfs.
//! Only Linux is read so far (ADR-035): elsewhere no drive is known to be
//! one the user can unplug, so nothing is written through here. Windows
//! writes a removable drive through itself (its default policy, quick
//! removal); a Mac's is still to see (5b).
//!
//! A drive that can be unplugged is written through (`op::builtin`): a copy
//! is on it, not in memory, when the copy says it is done, as Windows does
//! for removable drives. Devices that take a file over USB (a keyboard's or
//! a printer's firmware) are made for that, and one written to only when it
//! is unmounted may reset before the writes end and lose them.
//!
//! Where a lookup leads ([`Here`]) is read from the mount table too: a path
//! a file names is looked up only where that stays on this machine.

use std::path::{Component, Path, PathBuf, Prefix};
#[cfg(target_os = "linux")]
use std::time::{Duration, Instant};

#[cfg(target_os = "linux")]
use parking_lot::Mutex;

/// The buses a drive is plugged into by hand, as `lsblk` counts them for
/// HOTPLUG (util-linux's `hotplug_subsystems`).
#[cfg(target_os = "linux")]
const PLUGGED: [&str; 5] = ["usb", "ieee1394", "firewire", "pcmcia", "ccw"];

/// How deep device mapper and RAID layers are followed to the disks.
#[cfg(target_os = "linux")]
const LAYERS: u8 = 8;

/// Whether `path` is on a drive the user can unplug: one whose disk says it
/// is removable, or sits on USB, Firewire, PCMCIA or an SD card, through any
/// device mapper (LUKS) or RAID layer. Reads sysfs: on a worker. A
/// filesystem mounted from no block device (FUSE, NFS) is not known to be
/// on one.
#[cfg(target_os = "linux")]
pub fn is_removable(path: &Path) -> bool {
    let Ok(stat) = rustix::fs::stat(path) else {
        return false;
    };
    // Asked once per step of a copy: an answer is kept a moment.
    let now = Instant::now();
    let mut known = KNOWN.lock();
    known.retain(|k| now.duration_since(k.asked) < KEEP);
    if let Some(k) = known.iter().find(|k| k.device == stat.st_dev) {
        return k.removable;
    }
    drop(known);
    let removable = look_up(path, stat.st_dev);
    KNOWN.lock().push(Known {
        device: stat.st_dev,
        removable,
        asked: now,
    });
    removable
}

/// The bytes free for the user on the volume `path` is on, as the system
/// says (`statvfs`'s blocks available, Windows' bytes free to the caller):
/// what a copy there may write. `None` when it cannot say. A network share
/// may say 0, or more than there is: a copy asks rather than refuses
/// (`op::room`). I/O: a worker's.
pub fn free(path: &Path) -> Option<u64> {
    #[cfg(unix)]
    {
        let st = rustix::fs::statvfs(path).ok()?;
        Some(st.f_bavail.saturating_mul(st.f_frsize))
    }
    #[cfg(windows)]
    {
        super::win::free_space(path)
    }
}

/// Whether the volume `path` is on is mounted read-only (`statvfs`'s
/// `ST_RDONLY`): nothing on it can change, its access included. `None`
/// where the system is not asked yet (Windows). I/O: a worker's.
pub fn read_only(path: &Path) -> Option<bool> {
    #[cfg(unix)]
    {
        let st = rustix::fs::statvfs(path).ok()?;
        Some(st.f_flag.contains(rustix::fs::StatVfsMountFlags::RDONLY))
    }
    #[cfg(windows)]
    {
        let _ = path;
        None
    }
}

/// [`is_removable`] where only Linux is read yet: never known.
#[cfg(not(target_os = "linux"))]
pub fn is_removable(_: &Path) -> bool {
    false
}

/// How long an answer of [`is_removable`] is kept, by device number.
#[cfg(target_os = "linux")]
const KEEP: Duration = Duration::from_secs(5);

#[cfg(target_os = "linux")]
struct Known {
    device: u64,
    removable: bool,
    asked: Instant,
}

#[cfg(target_os = "linux")]
static KNOWN: Mutex<Vec<Known>> = Mutex::new(Vec::new());

#[cfg(target_os = "linux")]
fn look_up(path: &Path, device: u64) -> bool {
    let sys = Path::new("/sys");
    let own = number(device);
    if sys.join("dev/block").join(&own).exists() {
        return removable_in(sys, &own);
    }
    // btrfs numbers its files' device apart from its disks: the disk is
    // the device mounted.
    mounted_from(path).is_some_and(|device| device_removable(&device))
}

/// [`is_removable`] for the block device `source` (`/dev/sdb1`, as
/// mountinfo names what a mount holds). Reads `/dev` and sysfs only, never
/// the filesystem mounted, so a sleeping or unreachable volume cannot hold
/// the caller.
#[cfg(target_os = "linux")]
pub fn device_removable(source: &Path) -> bool {
    rustix::fs::stat(source)
        .ok()
        .filter(|d| {
            rustix::fs::FileType::from_raw_mode(d.st_mode) == rustix::fs::FileType::BlockDevice
        })
        .is_some_and(|d| removable_in(Path::new("/sys"), &number(d.st_rdev)))
}

/// [`device_removable`] where only Linux is read yet: never known.
#[cfg(not(target_os = "linux"))]
pub fn device_removable(_: &Path) -> bool {
    false
}

#[cfg(target_os = "linux")]
fn number(device: u64) -> String {
    format!(
        "{}:{}",
        rustix::fs::major(device),
        rustix::fs::minor(device)
    )
}

/// The device mounted where `path` is, as mountinfo names it.
#[cfg(target_os = "linux")]
fn mounted_from(path: &Path) -> Option<PathBuf> {
    let path = std::fs::canonicalize(path).ok()?;
    let mountinfo = std::fs::read("/proc/self/mountinfo").ok()?;
    source_of(&path, &mountinfo)
}

/// Whether only this machine writes the filesystem holding `path`: it is
/// mounted from one of its block devices (a disk, a stick, an `ntfs-3g`
/// drive), not shared over a network, so a lock taken on a file there is
/// seen by everything that could write to it. Reads mountinfo: on a
/// worker.
#[cfg(target_os = "linux")]
pub fn only_here(path: &Path) -> bool {
    let (Ok(path), Ok(mountinfo)) = (
        std::fs::canonicalize(path),
        std::fs::read("/proc/self/mountinfo"),
    ) else {
        return false;
    };
    only_here_in(&path, &mountinfo)
}

/// [`only_here`] where only Linux is read yet: never known.
#[cfg(not(target_os = "linux"))]
pub fn only_here(_: &Path) -> bool {
    false
}

/// [`only_here`] for `path` in `mountinfo`. The filesystems of a cluster
/// are mounted from a disk several machines share.
#[cfg(target_os = "linux")]
fn only_here_in(path: &Path, mountinfo: &(impl AsRef<[u8]> + ?Sized)) -> bool {
    mount_of(path, mountinfo).is_some_and(|mount| {
        Path::new(&mount.source).starts_with("/dev/")
            && !matches!(mount.fstype.as_str(), "gfs2" | "ocfs2")
    })
}

/// One line of `/proc/self/mountinfo`: a mount.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Mounted {
    /// What of its filesystem is mounted: `/` for all of it, a subvolume
    /// or a folder of it otherwise (`/@home`).
    pub root: PathBuf,
    /// Where it is mounted.
    pub point: PathBuf,
    pub fstype: String,
    /// What is mounted (`/dev/sdb1`, `user@host:/`), as the kernel says.
    pub source: std::ffi::OsString,
    /// The device its files are on (`major:minor`), what a stat of them
    /// says; `None` where the line gives none.
    pub device: Option<u64>,
    /// Its number in the table, which a later mount may take again once
    /// it is let go of.
    pub id: Option<u64>,
}

/// Every mount `mountinfo` (the bytes of `/proc/self/mountinfo`) lists,
/// in its order, its paths exact whatever bytes they hold. The one reading
/// of it: the places plugin's volumes and the write-through's devices.
/// Bytes, not text: the kernel escapes only a space, a tab, a line end and
/// a backslash, and writes every other byte of a path as it is, so one
/// mount point anywhere whose name is not UTF-8 made the whole read fail.
pub fn mountinfo(text: &(impl AsRef<[u8]> + ?Sized)) -> Vec<Mounted> {
    text.as_ref()
        .split(|b| *b == b'\n')
        .filter_map(|line| {
            // 36 35 98:0 /mnt1 /mnt2 rw,noatime master:1 - ext3 /dev/root rw
            let at = line.windows(3).position(|w| w == b" - ")?;
            let (own, rest) = (&line[..at], &line[at + 3..]);
            let mut own = own.split(|b| *b == b' ');
            let id = own
                .next()
                .and_then(|id| std::str::from_utf8(id).ok()?.parse().ok());
            let device = own.nth(1).and_then(device_number);
            let root = own.next()?;
            let point = own.next()?;
            let mut rest = rest.split(|b| *b == b' ');
            Some(Mounted {
                root: PathBuf::from(unescape(root)),
                point: PathBuf::from(unescape(point)),
                fstype: String::from_utf8_lossy(rest.next()?).into_owned(),
                source: unescape(rest.next().unwrap_or_default()),
                device,
                id,
            })
        })
        .collect()
}

/// The mount points at `path` or below it, as the system's table has them
/// (a remote's included: each GVfs mount a folder of its bridge): what a
/// removal of `path` would reach into, a drive's files or a server's.
/// Compared by the paths' text, never by reading them: from any thread.
pub fn mounted_within(path: &Path) -> Vec<PathBuf> {
    let mut points: Vec<PathBuf> = Here::now()
        .mounts
        .unwrap_or_default()
        .into_iter()
        .map(|(point, _)| point)
        .filter(|point| point.starts_with(path))
        .collect();
    if let Some(remote) = crate::fs::reach::of(path)
        && remote.point == path
        && !points.contains(&remote.point)
    {
        points.push(remote.point.clone());
    }
    points
}

/// What looking a path up reaches: this machine, and the server the user
/// is on already. A path written in a file or printed in the terminal is
/// looked up only where [`Here::holds`] says so (ADR-029): on Windows a
/// name like `\\host\share` connects to that host and offers it the user's
/// credentials; below an automount point nothing is mounted on yet
/// (autofs: macOS's `/net` and `/home`, Linux's `-hosts` maps and
/// systemd's automount units) a lookup mounts what the name says, from
/// another machine for `/net/host`. A link on the disk is followed as
/// listing follows it: only the path as written is read here.
#[derive(Clone, Debug)]
pub struct Here {
    /// Every mount point, and whether it is an automount point, in the
    /// table's order; `None` when the table could not be read.
    mounts: Option<Vec<(PathBuf, bool)>>,
}

impl Here {
    /// The mounts as the system has them now. Reads the mount table, never
    /// a path; on a worker all the same.
    #[must_use]
    pub fn now() -> Self {
        #[cfg(target_os = "linux")]
        let mounts = std::fs::read("/proc/self/mountinfo")
            .ok()
            .map(|text| automounts(&text));
        #[cfg(target_os = "macos")]
        let mounts = mac::mounts();
        // Windows names a server by its path, which `holds` reads.
        #[cfg(not(any(target_os = "linux", target_os = "macos")))]
        let mounts = Some(Vec::new());
        Self { mounts }
    }

    /// The automount points `points`, nothing mounted on them: a table read
    /// elsewhere, or a test's.
    #[must_use]
    pub fn with_automounts(points: impl IntoIterator<Item = PathBuf>) -> Self {
        Self {
            mounts: Some(points.into_iter().map(|point| (point, true)).collect()),
        }
    }

    /// Whether looking `path` up from `from`, the folder the user is in (the
    /// document's, the shell's), stays on this machine or on the server
    /// `from` is on. Both absolute, `.` and `..` worked out by their text.
    /// A table that could not be read leaves only what is below `from`.
    #[must_use]
    pub fn holds(&self, path: &Path, from: &Path) -> bool {
        if let Some(Component::Prefix(prefix)) = path.components().next()
            && !matches!(prefix.kind(), Prefix::Disk(_) | Prefix::VerbatimDisk(_))
            && from.components().next() != Some(Component::Prefix(prefix))
        {
            return false;
        }
        let Some(mounts) = &self.mounts else {
            return path.starts_with(from);
        };
        // The deepest mount holding `path`, the last of those at one point:
        // the kernel stacks them, and a mount on an automount point is what
        // it mounted.
        let mut deepest: Option<&(PathBuf, bool)> = None;
        for mount in mounts.iter().filter(|(point, _)| path.starts_with(point)) {
            if deepest.is_none_or(|(point, _)| mount.0.as_os_str().len() >= point.as_os_str().len())
            {
                deepest = Some(mount);
            }
        }
        // The point itself is read without mounting anything (`stat` does
        // not automount the name it ends with); what is below it is not.
        !deepest.is_some_and(|(point, automount)| *automount && path != point)
    }
}

/// Every mount point `mountinfo` lists, and whether it is an automount point.
#[cfg(target_os = "linux")]
fn automounts(mountinfo: &(impl AsRef<[u8]> + ?Sized)) -> Vec<(PathBuf, bool)> {
    self::mountinfo(mountinfo)
        .into_iter()
        .map(|mount| (mount.point, mount.fstype == "autofs"))
        .collect()
}

/// macOS's mount table, read by `getfsstat`, which looks no path up.
#[cfg(target_os = "macos")]
mod mac {
    use std::os::unix::ffi::OsStrExt;
    use std::path::PathBuf;

    /// A name the kernel wrote into a fixed array, up to its NUL.
    fn name(chars: &[libc::c_char]) -> Vec<u8> {
        chars
            .iter()
            .map(|c| c.cast_unsigned())
            .take_while(|b| *b != 0)
            .collect()
    }

    pub(super) fn mounts() -> Option<Vec<(PathBuf, bool)>> {
        // SAFETY: no buffer: the call only counts the mounts.
        #[allow(unsafe_code)]
        let count = unsafe { libc::getfsstat(std::ptr::null_mut(), 0, libc::MNT_NOWAIT) };
        // Room for a few mounted meanwhile; more are read next time.
        let mut table: Vec<libc::statfs> = Vec::with_capacity(usize::try_from(count).ok()? + 8);
        let bytes = libc::c_int::try_from(table.capacity() * size_of::<libc::statfs>()).ok()?;
        // SAFETY: the buffer has room for `bytes` bytes; the call writes
        // whole records within them and says how many.
        #[allow(unsafe_code)]
        let filled = unsafe { libc::getfsstat(table.as_mut_ptr(), bytes, libc::MNT_NOWAIT) };
        let filled = usize::try_from(filled).ok()?.min(table.capacity());
        // SAFETY: the call initialized the first `filled` records.
        #[allow(unsafe_code)]
        unsafe {
            table.set_len(filled);
        }
        Some(
            table
                .iter()
                .map(|fs| {
                    let point = name(&fs.f_mntonname);
                    let point = PathBuf::from(std::ffi::OsStr::from_bytes(&point));
                    (point, name(&fs.f_fstypename) == b"autofs")
                })
                .collect(),
        )
    }
}

/// In `mountinfo`, the mount that holds `path`: the last of the deepest,
/// as the kernel stacks them.
#[cfg(target_os = "linux")]
fn mount_of(path: &Path, mountinfo: &(impl AsRef<[u8]> + ?Sized)) -> Option<Mounted> {
    let mut best: Option<Mounted> = None;
    for mount in self::mountinfo(mountinfo) {
        if path.starts_with(&mount.point)
            && best
                .as_ref()
                .is_none_or(|b| mount.point.as_os_str().len() >= b.point.as_os_str().len())
        {
            best = Some(mount);
        }
    }
    best
}

/// In `mountinfo`, the device mounted where `path` is.
#[cfg(target_os = "linux")]
fn source_of(path: &Path, mountinfo: &(impl AsRef<[u8]> + ?Sized)) -> Option<PathBuf> {
    let source = PathBuf::from(mount_of(path, mountinfo)?.source);
    source.starts_with("/dev/").then_some(source)
}

/// A mount's device as mountinfo writes it, `major:minor`, as a stat's
/// device number (glibc's `makedev`).
fn device_number(field: &[u8]) -> Option<u64> {
    let (major, minor) = std::str::from_utf8(field).ok()?.split_once(':')?;
    let (major, minor): (u64, u64) = (major.parse().ok()?, minor.parse().ok()?);
    Some(
        ((major & 0xffff_f000) << 32)
            | ((major & 0x0000_0fff) << 8)
            | ((minor & 0xffff_ff00) << 12)
            | (minor & 0x0000_00ff),
    )
}

/// mountinfo writes a space, a tab, a newline and a backslash as `\040`…
fn unescape(field: &[u8]) -> std::ffi::OsString {
    let b = field;
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'\\'
            && let Some(v) = b
                .get(i + 1..i + 4)
                .and_then(|d| std::str::from_utf8(d).ok())
                .and_then(|d| u8::from_str_radix(d, 8).ok())
        {
            out.push(v);
            i += 4;
            continue;
        }
        out.push(b[i]);
        i += 1;
    }
    crate::os_path::from_bytes(out)
}

/// [`is_removable`] for the block device `major:minor` of the sysfs at `sys`.
#[cfg(target_os = "linux")]
fn removable_in(sys: &Path, device: &str) -> bool {
    let Ok(sys) = std::fs::canonicalize(sys) else {
        return false;
    };
    std::fs::canonicalize(sys.join("dev/block").join(device))
        .is_ok_and(|dir| block_removable(&sys, &dir, 0))
}

#[cfg(target_os = "linux")]
fn block_removable(sys: &Path, dir: &Path, depth: u8) -> bool {
    // A mapped or RAID device is on whatever its members are on.
    if depth < LAYERS
        && let Ok(members) = std::fs::read_dir(dir.join("slaves"))
    {
        let members: Vec<PathBuf> = members
            .filter_map(|m| std::fs::canonicalize(m.ok()?.path()).ok())
            .collect();
        if !members.is_empty() {
            return members.iter().any(|m| block_removable(sys, m, depth + 1));
        }
    }
    // A partition's flags are its disk's.
    let disk = match dir.parent() {
        Some(parent) if dir.join("partition").exists() => parent,
        _ => dir,
    };
    if read(&disk.join("removable")).as_deref() == Some("1") {
        return true;
    }
    let devices = sys.join("devices");
    disk.ancestors()
        .take_while(|a| a.starts_with(&devices) && *a != devices)
        .any(|a| {
            let subsystem = std::fs::read_link(a.join("subsystem")).ok();
            match subsystem.as_deref().and_then(Path::file_name) {
                Some(s) if PLUGGED.iter().any(|p| s == *p) => true,
                // An SD card, not the eMMC a laptop may boot from.
                Some(s) if s == "mmc" => read(&a.join("type")).is_some_and(|t| t != "MMC"),
                _ => false,
            }
        })
}

#[cfg(target_os = "linux")]
fn read(path: &Path) -> Option<String> {
    std::fs::read_to_string(path)
        .ok()
        .map(|s| s.trim().to_owned())
}

#[cfg(all(test, target_os = "linux"))]
mod tests {
    use std::os::unix::fs::symlink;

    use super::*;

    /// A sysfs of synthetic devices under the tests' cache folder.
    struct Sys(tempfile::TempDir);

    impl Sys {
        fn new() -> Self {
            let base = dirs::home_dir().unwrap().join(".cache/skwir-tests");
            std::fs::create_dir_all(&base).unwrap();
            let dir = tempfile::tempdir_in(base).unwrap();
            for d in [
                "dev/block",
                "devices",
                "bus/usb",
                "bus/mmc",
                "bus/pci",
                "bus/scsi",
            ] {
                std::fs::create_dir_all(dir.path().join(d)).unwrap();
            }
            Self(dir)
        }

        fn root(&self) -> &Path {
            self.0.path()
        }

        /// A device directory `path` under `devices`, of `subsystem`.
        fn device(&self, path: &str, subsystem: Option<&str>) -> PathBuf {
            let dir = self.root().join("devices").join(path);
            std::fs::create_dir_all(&dir).unwrap();
            if let Some(s) = subsystem {
                symlink(self.root().join("bus").join(s), dir.join("subsystem")).unwrap();
            }
            dir
        }

        /// A block device at `path`, numbered `number`: a partition when
        /// its parent is a disk.
        fn block(&self, path: &str, number: &str, removable: Option<&str>) -> PathBuf {
            let dir = self.device(path, None);
            if let Some(r) = removable {
                std::fs::write(dir.join("removable"), format!("{r}\n")).unwrap();
            }
            symlink(&dir, self.root().join("dev/block").join(number)).unwrap();
            dir
        }

        fn partition(&self, path: &str, number: &str) {
            let dir = self.block(path, number, None);
            std::fs::write(dir.join("partition"), "1\n").unwrap();
        }

        fn removable(&self, number: &str) -> bool {
            removable_in(self.root(), number)
        }
    }

    #[test]
    fn nothing_below_an_automount_point_is_looked_up() {
        let mountinfo = "\
22 1 0:30 /@ / rw - btrfs /dev/mapper/root rw
30 22 0:40 / /net rw - autofs -hosts rw,fd=5
31 22 0:41 / /boot rw - autofs systemd-1 rw,fd=6
32 31 259:1 / /boot rw - vfat /dev/nvme0n1p1 rw
33 30 0:42 / /net/nas/export rw - nfs4 nas:/export rw
34 22 0:43 / /mnt/archive rw - autofs systemd-1 rw,fd=7
";
        let here = Here {
            mounts: Some(automounts(mountinfo)),
        };
        let from = Path::new("/home/example");
        let holds = |path: &str| here.holds(Path::new(path), from);
        assert!(holds("/home/example/notes.md"));
        assert!(holds("/etc/hosts"));
        // The points themselves, not what a lookup there would mount.
        assert!(holds("/net"));
        assert!(holds("/mnt/archive"));
        assert!(!holds("/net/elsewhere"));
        assert!(!holds("/net/elsewhere/notes.md"));
        assert!(!holds("/mnt/archive/2025"));
        // Mounted already.
        assert!(holds("/net/nas/export/notes.md"));
        assert!(!holds("/net/nas/other"));
        assert!(holds("/boot/efi/loader.conf"));

        let unread = Here { mounts: None };
        assert!(unread.holds(Path::new("/home/example/a/b.md"), from));
        assert!(!unread.holds(Path::new("/etc/hosts"), from));
    }

    #[test]
    fn a_path_is_on_the_deepest_mount_that_holds_it() {
        let mountinfo = "\
22 1 0:30 /@ / rw - btrfs /dev/mapper/root rw
40 22 0:52 / /run/media/example/Back\\040up rw,nosuid - btrfs /dev/mapper/luks-example rw
41 22 0:53 / /run/media/example/net rw - fuse.sshfs host:/share rw
";
        let source = |p: &str| source_of(Path::new(p), mountinfo);
        assert_eq!(
            source("/run/media/example/Back up/2026"),
            Some(PathBuf::from("/dev/mapper/luks-example"))
        );
        assert_eq!(
            source("/home/example"),
            Some(PathBuf::from("/dev/mapper/root"))
        );
        assert_eq!(source("/run/media/example/net/x"), None, "not a device");
        assert_eq!(
            source("/run/media/example/Back"),
            Some(PathBuf::from("/dev/mapper/root")),
            "a name that only begins like the mount's is not in it"
        );
    }

    #[test]
    fn only_a_filesystem_mounted_from_a_device_here_is_written_only_here() {
        let mountinfo = "\
22 1 0:30 /@ / rw - btrfs /dev/mapper/root rw
40 22 8:49 / /run/media/example/STICK rw - vfat /dev/sdd1 rw
41 22 0:53 / /run/media/example/net rw - fuse.sshfs host:/share rw
42 22 0:54 / /mnt/nas rw - nfs4 nas.example:/photos rw
43 22 8:65 / /mnt/old rw - fuseblk /dev/sde1 rw
44 22 253:3 / /mnt/cluster rw - gfs2 /dev/mapper/shared rw
45 22 0:40 / /tmp rw - tmpfs tmpfs rw
";
        let here = |p: &str| only_here_in(Path::new(p), mountinfo);
        assert!(here("/home/example"));
        assert!(here("/run/media/example/STICK/DCIM"));
        assert!(here("/mnt/old/a"), "ntfs-3g, on a disk of this machine");
        assert!(!here("/run/media/example/net/a"));
        assert!(!here("/mnt/nas/2026"));
        assert!(!here("/mnt/cluster/a"), "a disk machines share");
        assert!(!here("/tmp/a"), "no device to tell");
    }

    #[test]
    fn a_usb_stick_and_its_partition_are_removable() {
        let sys = Sys::new();
        sys.device("pci0000:00", Some("pci"));
        sys.device("pci0000:00/usb9/9-1", Some("usb"));
        sys.block("pci0000:00/usb9/9-1/host3/block/sdd", "8:48", Some("0"));
        sys.partition("pci0000:00/usb9/9-1/host3/block/sdd/sdd1", "8:49");
        assert!(sys.removable("8:48"));
        assert!(sys.removable("8:49"), "a partition is on its disk's bus");
    }

    #[test]
    fn an_internal_disk_is_not() {
        let sys = Sys::new();
        sys.device("pci0000:00", Some("pci"));
        sys.block("pci0000:00/nvme/nvme0/nvme0n1", "259:0", Some("0"));
        sys.partition("pci0000:00/nvme/nvme0/nvme0n1/nvme0n1p2", "259:2");
        assert!(!sys.removable("259:2"));
        assert!(!sys.removable("1:99"), "no such device");
    }

    #[test]
    fn a_disk_that_says_it_is_removable_is() {
        let sys = Sys::new();
        sys.block("pci0000:00/ata1/host0/block/sr0", "11:0", Some("1"));
        assert!(sys.removable("11:0"));
    }

    #[test]
    fn a_container_is_on_its_member_drive() {
        let sys = Sys::new();
        sys.device("pci0000:00/usb2/2-1", Some("usb"));
        sys.block("pci0000:00/usb2/2-1/host5/block/sdb", "8:16", Some("0"));
        sys.partition("pci0000:00/usb2/2-1/host5/block/sdb/sdb2", "8:18");
        let dm = sys.block("virtual/block/dm-2", "254:2", Some("0"));
        std::fs::create_dir(dm.join("slaves")).unwrap();
        symlink(
            sys.root()
                .join("devices/pci0000:00/usb2/2-1/host5/block/sdb/sdb2"),
            dm.join("slaves/sdb2"),
        )
        .unwrap();
        assert!(sys.removable("254:2"));

        let root = sys.block("virtual/block/dm-0", "254:0", Some("0"));
        std::fs::create_dir(root.join("slaves")).unwrap();
        sys.block("pci0000:00/nvme/nvme0n1", "259:0", Some("0"));
        symlink(
            sys.root().join("devices/pci0000:00/nvme/nvme0n1"),
            root.join("slaves/nvme0n1"),
        )
        .unwrap();
        assert!(!sys.removable("254:0"));
    }

    #[test]
    fn an_sd_card_is_removable_and_emmc_is_not() {
        let sys = Sys::new();
        let card = sys.device("pci0000:00/mmc_host/mmc0/mmc0:aaaa", Some("mmc"));
        std::fs::write(card.join("type"), "SD\n").unwrap();
        sys.block(
            "pci0000:00/mmc_host/mmc0/mmc0:aaaa/block/mmcblk0",
            "179:0",
            Some("0"),
        );
        assert!(sys.removable("179:0"));

        let emmc = sys.device("pci0000:00/mmc_host/mmc1/mmc1:0001", Some("mmc"));
        std::fs::write(emmc.join("type"), "MMC\n").unwrap();
        sys.block(
            "pci0000:00/mmc_host/mmc1/mmc1:0001/block/mmcblk1",
            "179:8",
            Some("0"),
        );
        assert!(!sys.removable("179:8"));
    }
}

#[cfg(all(test, windows))]
mod windows_tests {
    use super::*;

    /// A server named by a path is reached only from a folder on its share:
    /// the user is there already.
    #[test]
    fn a_server_is_looked_up_only_from_its_own_share() {
        let here = Here::now();
        let from = Path::new(r"C:\Users\example");
        let holds = |path: &str, from: &Path| here.holds(Path::new(path), from);
        assert!(holds(r"C:\Users\example\notes.md", from));
        assert!(holds(r"D:\notes.md", from));
        assert!(holds(r"\\?\C:\notes.md", from));
        assert!(!holds(r"\\host\share\notes.md", from));
        assert!(!holds("//host/share/notes.md", from));
        assert!(!holds(r"\\?\UNC\host\share\notes.md", from));
        assert!(!holds(r"\\.\pipe\example", from));
        let share = Path::new(r"\\host\share\docs");
        assert!(holds(r"\\host\share\other\notes.md", share));
        assert!(!holds(r"\\elsewhere\share\notes.md", share));
    }
}
