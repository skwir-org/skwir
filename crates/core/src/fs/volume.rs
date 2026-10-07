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

use std::path::{Path, PathBuf};
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
    let mountinfo = std::fs::read_to_string("/proc/self/mountinfo").ok()?;
    source_of(&path, &mountinfo)
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
}

/// Every mount `mountinfo` (the text of `/proc/self/mountinfo`) lists, in
/// its order, its paths exact whatever bytes they hold. The one reading of
/// it: the places plugin's volumes and the write-through's devices.
pub fn mountinfo(text: &str) -> Vec<Mounted> {
    text.lines()
        .filter_map(|line| {
            // 36 35 98:0 /mnt1 /mnt2 rw,noatime master:1 - ext3 /dev/root rw
            let (own, rest) = line.split_once(" - ")?;
            let mut own = own.split(' ');
            let root = own.nth(3)?;
            let point = own.next()?;
            let mut rest = rest.split(' ');
            Some(Mounted {
                root: PathBuf::from(unescape(root)),
                point: PathBuf::from(unescape(point)),
                fstype: rest.next()?.to_owned(),
                source: unescape(rest.next().unwrap_or_default()),
            })
        })
        .collect()
}

/// In `mountinfo`, the source of the mount that holds `path`: the last of
/// the deepest, as the kernel stacks them.
#[cfg(target_os = "linux")]
fn source_of(path: &Path, mountinfo: &str) -> Option<PathBuf> {
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
    let source = PathBuf::from(best?.source);
    source.starts_with("/dev/").then_some(source)
}

/// mountinfo writes a space, a tab, a newline and a backslash as `\040`…
fn unescape(field: &str) -> std::ffi::OsString {
    let b = field.as_bytes();
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
            let base = dirs::home_dir().unwrap().join(".cache/bibli-tests");
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
