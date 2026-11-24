//! Places the user already knows: home, XDG user dirs, GTK bookmarks, mounts.

use std::path::{Path, PathBuf};
use std::time::Duration;

use skwir_core::CancelToken;
use skwir_core::words;
use skwir_core::words::Words;

/// An XDG user directory.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct UserDir {
    /// Display name (last path component).
    pub name: String,
    pub path: PathBuf,
    /// `desktop`, `download`, `documents`, `music`, `pictures`, `videos`,
    /// `templates`, `publicshare`, or the lowercase key for custom entries.
    pub kind: String,
}

/// A GTK sidebar bookmark.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Bookmark {
    pub title: String,
    pub path: PathBuf,
}

/// A mounted volume worth showing.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Mount {
    /// What it is called, said where it is shown: its folder's name as
    /// written, or Skwir's words for the volume the system runs from
    /// ("Root") and a Windows drive ("Local Disk (C:)").
    pub label: Words,
    pub path: PathBuf,
    pub fstype: String,
    /// Mounted by udisks for the user (under `/run/media` or `/media`):
    /// what Eject lets go of. System volumes are not ours to detach.
    pub udisks: bool,
    /// On a drive the user can unplug (`fs::volume::device_removable`:
    /// sysfs, as `lsblk` says HOTPLUG), wherever it is mounted; filled by
    /// `mounts`, false from the parser alone.
    pub removable: bool,
    /// The mounted device (`/dev/sdb1`), as the kernel reports it.
    pub source: String,
    /// What of its filesystem is mounted there: `/` for all of it, a
    /// subvolume or a folder of it otherwise (`/@home`).
    pub root: PathBuf,
    /// Its volume is locked, though its place is there: a Windows drive
    /// BitLocker keeps locked (`windows::drive_locked`), its letter given
    /// and nothing readable. Opening it unlocks it ([`unlock_by_system`]).
    pub locked: bool,
}

/// Free and total bytes of the filesystem holding `path`. Asks the
/// filesystem, which can block on a sleeping or unreachable volume: call it
/// from a worker.
pub fn space(path: &Path) -> Option<(u64, u64)> {
    #[cfg(unix)]
    {
        let stat = rustix::fs::statvfs(path).ok()?;
        let total = stat.f_blocks.checked_mul(stat.f_frsize)?;
        (total > 0).then(|| (stat.f_bavail.saturating_mul(stat.f_frsize), total))
    }
    #[cfg(windows)]
    {
        crate::windows::space(path)
    }
}

/// A volume that is present but not mounted.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Volume {
    /// Device to hand to udisks (`/dev/sdb1`, `/dev/mapper/luks-…`).
    pub device: String,
    pub label: String,
    pub size: u64,
    /// Encrypted and locked: it needs a passphrase before it can be mounted.
    pub locked: bool,
    pub hotplug: bool,
    /// The locked container it is the cleartext of (`/dev/sdb2` for
    /// `/dev/mapper/luks-…`), as `lsblk` names it: once unlocked, a
    /// container is listed as this volume instead.
    pub container: Option<String>,
    /// A small FAT or NTFS partition: a boot or recovery one, by the
    /// judgement desktops make where udisks says nothing (`Identity`).
    pub system: bool,
    /// What the system's own listing names it by, where udev does not (a
    /// Mac's APFS volume UUID); on Linux `None`, and [`identity`] says.
    pub uuid: Option<String>,
}

/// What udev knows of a block device, read from sysfs and udev's
/// database: both in memory, never a drive, so it can be asked anywhere.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Identity {
    /// Its filesystem's UUID; for a container's cleartext volume the
    /// container's, so that the volume is one, locked or not.
    pub uuid: Option<String>,
    /// Whether udisks says not to show it (`UDISKS_IGNORE`, which its
    /// rules set for EFI, BIOS boot, recovery and reserved partitions);
    /// `None` when udev has no record of it.
    pub ignored: Option<bool>,
}

/// What udev knows of `device` (`/dev/sdb1`, `/dev/mapper/luks-…`).
pub fn identity(device: &str) -> Identity {
    // The kernel's name for it: `/dev/mapper/luks-…` leads to `/dev/dm-3`.
    if !device.starts_with("/dev/") {
        return Identity::default();
    }
    let Some(name) = std::fs::canonicalize(device)
        .ok()
        .and_then(|node| node.file_name().map(ToOwned::to_owned))
    else {
        return Identity::default();
    };
    block_identity(
        Path::new("/sys/class/block"),
        Path::new("/run/udev/data"),
        &name,
    )
}

/// [`identity`] of the block device the kernel names `name`, from the
/// sysfs and udev trees at `sys` and `udev`.
fn block_identity(sys: &Path, udev: &Path, name: &std::ffi::OsStr) -> Identity {
    let dir = sys.join(name);
    let own = std::fs::read_to_string(dir.join("dev"))
        .ok()
        .and_then(|dev| std::fs::read_to_string(udev.join(format!("b{}", dev.trim()))).ok())
        .map(|record| udev_identity(&record))
        .unwrap_or_default();
    // A container's cleartext volume (device mapper's `CRYPT-…`) is the
    // container under it: the one device it is made of.
    let crypt =
        std::fs::read_to_string(dir.join("dm/uuid")).is_ok_and(|uuid| uuid.starts_with("CRYPT-"));
    let container = crypt
        .then(|| std::fs::read_dir(dir.join("slaves")).ok()?.flatten().next())
        .flatten()
        .map(|slave| block_identity(sys, udev, &slave.file_name()));
    match container {
        Some(container) => Identity {
            uuid: container.uuid.or(own.uuid),
            ignored: match (container.ignored, own.ignored) {
                (None, None) => None,
                (a, b) => Some(a == Some(true) || b == Some(true)),
            },
        },
        None => own,
    }
}

/// The properties of a udev record (`E:ID_FS_UUID=…` lines) that say
/// what a volume is: a record without `UDISKS_IGNORE` says to show it.
fn udev_identity(record: &str) -> Identity {
    let mut identity = Identity {
        uuid: None,
        ignored: Some(false),
    };
    for (key, value) in record
        .lines()
        .filter_map(|line| line.strip_prefix("E:")?.split_once('='))
    {
        match key {
            "ID_FS_UUID" if !value.is_empty() => identity.uuid = Some(value.to_owned()),
            "UDISKS_IGNORE" => identity.ignored = Some(value == "1"),
            _ => {}
        }
    }
    identity
}

#[derive(serde::Deserialize)]
struct BlockDevices {
    blockdevices: Vec<BlockDevice>,
}

#[derive(serde::Deserialize)]
struct BlockDevice {
    path: String,
    label: Option<String>,
    size: Option<u64>,
    fstype: Option<String>,
    mountpoint: Option<String>,
    #[serde(default)]
    hotplug: bool,
    pkname: Option<String>,
}

const LSBLK_ARGS: [&str; 5] = [
    "--json",
    "--bytes",
    "--list",
    "--output",
    "PATH,LABEL,SIZE,FSTYPE,MOUNTPOINT,HOTPLUG,PKNAME",
];

/// Below this a FAT or NTFS partition is a boot or recovery partition, which
/// desktops hide too, where udisks does not say (`Identity::ignored`).
const SYSTEM_PARTITION_MAX: u64 = 2 << 30;

fn device_name(path: &str) -> &str {
    path.rsplit('/').next().unwrap_or(path)
}

/// Volumes from `lsblk --json --list` that could be mounted and are not:
/// filesystems without a mount point, and locked encrypted containers.
/// Members of something else (swap, RAID, LVM, an unlocked container) are
/// left out; small system partitions are marked (`Volume::system`).
pub fn parse_lsblk(json: &str) -> Vec<Volume> {
    let Ok(listing) = serde_json::from_str::<BlockDevices>(json) else {
        return Vec::new();
    };
    let devices = listing.blockdevices;
    devices
        .iter()
        .filter_map(|device| {
            let fstype = device.fstype.as_deref()?;
            if device.mountpoint.is_some()
                || matches!(
                    fstype,
                    "swap" | "LVM2_member" | "linux_raid_member" | "zfs_member"
                )
            {
                return None;
            }
            let size = device.size.unwrap_or(0);
            let system = matches!(fstype, "vfat" | "ntfs") && size < SYSTEM_PARTITION_MAX;
            let locked = fstype == "crypto_LUKS";
            // An unlocked container shows up again as its cleartext device.
            let name = device_name(&device.path);
            if locked
                && devices
                    .iter()
                    .any(|other| other.pkname.as_deref() == Some(name))
            {
                return None;
            }
            // A cleartext device inherits "hotplug" from the disk under it.
            let hotplug = device.hotplug
                || device.pkname.as_ref().is_some_and(|parent| {
                    devices
                        .iter()
                        .any(|other| device_name(&other.path) == parent && other.hotplug)
                });
            let container = device.pkname.as_ref().and_then(|parent| {
                devices
                    .iter()
                    .find(|other| {
                        device_name(&other.path) == parent
                            && other.fstype.as_deref() == Some("crypto_LUKS")
                    })
                    .map(|other| other.path.clone())
            });
            Some(Volume {
                device: device.path.clone(),
                label: device.label.clone().unwrap_or_default(),
                size,
                locked,
                hotplug,
                container,
                system,
                uuid: None,
            })
        })
        .collect()
}

/// The encrypted device under a cleartext one (`/dev/sdb2` for
/// `/dev/mapper/luks-…`), to lock it after unmounting. The kernel may name
/// the cleartext device `/dev/dm-3`; both spellings resolve to the same node.
pub fn container_of(json: &str, cleartext: &str) -> Option<String> {
    let listing = serde_json::from_str::<BlockDevices>(json).ok()?;
    let resolved = std::fs::canonicalize(cleartext).ok();
    let device = listing.blockdevices.iter().find(|device| {
        device.path == cleartext
            || resolved.as_deref().is_some_and(|path| {
                std::fs::canonicalize(&device.path).ok().as_deref() == Some(path)
            })
    })?;
    let parent = device.pkname.as_ref()?;
    listing
        .blockdevices
        .iter()
        .find(|other| device_name(&other.path) == parent)
        .filter(|other| other.fstype.as_deref() == Some("crypto_LUKS"))
        .map(|other| other.path.clone())
}

/// How long `lsblk` may take: it reads sysfs and udev's database, which
/// answer at once unless something is badly wrong.
const LSBLK_TIMEOUT: Duration = Duration::from_secs(10);

/// How long udisks may take to mount, unmount, lock or unlock: a key
/// derivation takes seconds, flushing a slow stick before unmounting longer.
/// A drive that has not answered by then is not going to.
const UDISKS_TIMEOUT: Duration = Duration::from_secs(120);

/// Run `lsblk` and return its JSON. Spawns a process: call from a worker.
pub fn lsblk() -> Option<String> {
    let finished = crate::process::run_to_exit(
        crate::process::command("lsblk").args(LSBLK_ARGS),
        None,
        &CancelToken::new(),
        LSBLK_TIMEOUT,
    )
    .ok()?;
    finished
        .status
        .success()
        .then(|| String::from_utf8_lossy(&finished.stdout).into_owned())
}

/// Volumes present but not mounted, to mount or unlock from the sidebar:
/// `lsblk`'s (Linux); on a Mac the encrypted APFS volumes locked
/// (`apfs::locked`); on Windows none, a drive BitLocker locked being one of
/// its drives (`Mount::locked`). Spawns a process: call from a worker.
pub fn unmounted_volumes() -> Vec<Volume> {
    #[cfg(target_os = "macos")]
    {
        apfs::list()
            .map(|plist| apfs::locked(&plist))
            .unwrap_or_default()
    }
    #[cfg(windows)]
    {
        Vec::new()
    }
    #[cfg(not(any(target_os = "macos", windows)))]
    {
        lsblk().map(|json| parse_lsblk(&json)).unwrap_or_default()
    }
}

/// Run `udisksctl <verb> --block-device <device>`, with `input` on its
/// standard input, and return what it printed.
fn udisks(
    verb: &str,
    device: &str,
    extra: &[&str],
    input: Option<&[u8]>,
) -> anyhow::Result<String> {
    let mut command = crate::process::command("udisksctl");
    command
        .arg(verb)
        .arg("--block-device")
        .arg(device)
        .arg("--no-user-interaction")
        .args(extra);
    let finished =
        crate::process::run_to_exit(&mut command, input, &CancelToken::new(), UDISKS_TIMEOUT)
            .map_err(|error| {
                anyhow::Error::from(words!(
                    places_udisks_unrun = "udisksctl could not be run: { $error }",
                    error = error,
                    note = "udisksctl is the program that mounts and unlocks volumes (a name). error: why it could not be run."
                ))
            })?;
    // udisks says what went wrong on standard error, in the user's words.
    anyhow::ensure!(finished.status.success(), "{}", finished.stderr);
    Ok(String::from_utf8_lossy(&finished.stdout).into_owned())
}

/// `Mounted /dev/sdb1 at /run/media/example/USB stick` gives the mount point.
/// Older udisks end the sentence with a period, newer ones do not; a label
/// may end with one too, so the period goes only when the path without it is
/// the directory that exists.
pub fn mount_point_of(message: &str) -> Option<PathBuf> {
    let (_, path) = message.trim().split_once(" at ")?;
    let exact = PathBuf::from(path);
    match path.strip_suffix('.') {
        Some(shorter) if !exact.is_dir() && Path::new(shorter).is_dir() => {
            Some(PathBuf::from(shorter))
        }
        _ => Some(exact),
    }
}

/// `Unlocked /dev/sdb2 as /dev/dm-3.` gives the cleartext device.
pub fn cleartext_of(message: &str) -> Option<String> {
    let (_, device) = message.trim().split_once(" as ")?;
    Some(device.strip_suffix('.').unwrap_or(device).to_owned())
}

/// Mount through udisks (`diskutil` on a Mac) and return the mount point.
/// Blocks: worker only.
pub fn mount(device: &str) -> anyhow::Result<PathBuf> {
    #[cfg(target_os = "macos")]
    {
        apfs::mount(device)
    }
    #[cfg(windows)]
    {
        let _ = device;
        Err(skwir_core::system::not_yet_words(skwir_core::words!(
            places_mount_not_yet = "Mounting a volume is not available on { $system } yet",
            system = skwir_core::system::named(),
            note = "Why a volume shown in the sidebar is not mounted. system: the system's name (Windows)."
        ))
        .into())
    }
    #[cfg(not(any(target_os = "macos", windows)))]
    {
        let message = udisks("mount", device, &[], None)?;
        mount_point_of(&message).ok_or_else(|| {
            words!(
                places_udisks_no_mount_point = "udisksctl did not say where it mounted the volume",
                note = "udisksctl is the program that mounts volumes (a name)."
            )
            .into()
        })
    }
}

/// The system refused the passphrase given to unlock a volume
/// ([`is_refused`]): typed wrong, or kept from before it changed.
#[derive(Debug)]
pub struct Refused;

impl std::fmt::Display for Refused {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // Words, so that a chain said in the reader's language says it so.
        std::fmt::Display::fmt(
            &words!(
                places_passphrase_refused = "The passphrase was not accepted",
                note = "Why an encrypted volume was not unlocked: the passphrase was typed wrong, or has changed."
            ),
            f,
        )
    }
}

impl std::error::Error for Refused {}

/// Whether [`unlock`] failed because the passphrase was refused, which
/// another may answer, rather than for something it cannot.
pub fn is_refused(error: &anyhow::Error) -> bool {
    error.is::<Refused>()
}

/// Unlock an encrypted volume and return its cleartext device (a Mac's
/// APFS volume is its own, mounted by then). The passphrase travels on the
/// child's standard input, never on a command line, and is not kept here.
/// Windows asks for it itself ([`SYSTEM_ASKS`]). Blocks: worker only.
pub fn unlock(device: &str, passphrase: &str) -> anyhow::Result<String> {
    #[cfg(target_os = "macos")]
    {
        apfs::unlock(device, passphrase)
    }
    #[cfg(windows)]
    {
        let _ = (device, passphrase);
        anyhow::bail!(words!(
            places_bitlocker_asks =
                "Windows asks for a BitLocker drive's password itself: open the drive",
            note = "BitLocker is Windows' encryption of drives (a name)."
        ))
    }
    #[cfg(not(any(target_os = "macos", windows)))]
    {
        unlock_luks(device, passphrase)
    }
}

/// [`unlock`] through udisks.
#[cfg(not(any(target_os = "macos", windows)))]
fn unlock_luks(device: &str, passphrase: &str) -> anyhow::Result<String> {
    // No trailing newline: a key file is taken byte for byte.
    let message = udisks(
        "unlock",
        device,
        &["--key-file", "/dev/stdin"],
        Some(passphrase.as_bytes()),
    )
    .map_err(|error| {
        let text = error.to_string();
        if text.contains("Failed to activate") || text.contains("No key available") {
            anyhow::Error::new(Refused)
        } else {
            error
        }
    })?;
    cleartext_of(&message).ok_or_else(|| {
        words!(
            places_udisks_no_device = "udisksctl did not name the unlocked device",
            note = "udisksctl is the program that unlocks encrypted volumes (a name)."
        )
        .into()
    })
}

/// Whether the system asks for a locked volume's passphrase itself, in a
/// dialog of its own that offers to remember it (Windows' BitLocker): Skwir
/// then asks nothing and keeps nothing ([`unlock_by_system`], ADR-088).
pub const SYSTEM_ASKS: bool = cfg!(windows);

/// Unlock `device` through the system's own dialog, where it asks itself
/// ([`SYSTEM_ASKS`]), and answer where it is then, `None` when the user
/// left it locked or `cancel` came first. Blocks until the dialog closes:
/// worker only.
pub fn unlock_by_system(device: &str, cancel: &CancelToken) -> anyhow::Result<Option<PathBuf>> {
    #[cfg(windows)]
    {
        crate::windows::unlock_drive(device, cancel)
    }
    #[cfg(not(windows))]
    {
        let _ = (device, cancel);
        anyhow::bail!(words!(
            places_system_asks_not = "This system does not ask for a volume's passphrase itself",
            note = "Skwir asks for an encrypted volume's passphrase on this system; the system has no dialog of its own for it."
        ))
    }
}

/// Whether udisks refused because something still uses the volume
/// (`DeviceBusy`, "target is busy").
pub fn is_busy(error: &anyhow::Error) -> bool {
    let text = error.to_string();
    text.contains("DeviceBusy") || text.contains("target is busy")
}

/// Write to the drive what the filesystem mounted at `path` still holds in
/// memory, and say what could not be. Unmounting writes it too, but the
/// kernel takes the mount out of the list as the unmount begins, before
/// those writes, so the volume would look unmounted while the drive is
/// still being written, and a write lost then is not reported. Blocks for
/// as long as the drive takes: worker only.
pub fn flush(path: &Path) -> anyhow::Result<()> {
    // Closed before the unmount, which this would otherwise hold up.
    skwir_core::fs::Dir::open(path)?.sync_volume()?;
    Ok(())
}

/// What another program says of an unmount (`follow_unmounts`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum UnmountNotice {
    /// A desktop announced it before unmounting (GIO, Solid), and asks
    /// again while it is refused (GNOME's dialog retries).
    Announced(Vec<PathBuf>),
    /// udisks began it, asked by a program that announces nothing
    /// (`udisksctl`, udiskie): as the system unmounts, so what held the
    /// volume then made it refuse.
    Begun(Vec<PathBuf>),
    /// udisks's unmount of these was refused (busy, or anything else).
    Refused(Vec<PathBuf>),
}

/// Call `told` with what other programs say of unmounting a volume, until
/// `stop` (ADR-149, amended): on the session bus, GIO's volume monitor
/// before an unmount through it (GNOME's Eject, Nautilus, `gio mount -u`:
/// `MountPreUnmount`, the mount's root a `file://` URI) and KDE's Solid
/// before a teardown (Dolphin, the device notifier: `teardownRequested`,
/// the device's udi in its object path); on the system bus, udisks's
/// unmount jobs, whoever asked (`filesystem-unmount`, its block devices
/// in `Objects`, then `Completed`). On threads of their own, connecting
/// again when a bus is; `told` takes each at once and hands what may wait
/// to a worker. On Windows, what Windows says of removing, locking or
/// dismounting a drive a folder watch is on (`devices`, ADR-149 amended):
/// asked, `Announced`, the window answering once Skwir's watches there
/// are let go of; the removal refused or cancelled, `Refused`. macOS tells
/// nothing (`DiskArbitration`'s approval is not asked). Whether it follows.
pub fn follow_unmounts(
    stop: &CancelToken,
    told: impl Fn(UnmountNotice) + Send + Sync + 'static,
) -> bool {
    #[cfg(target_os = "linux")]
    {
        let told = std::sync::Arc::new(told);
        let session = {
            let (stop, told) = (stop.clone(), std::sync::Arc::clone(&told));
            std::thread::Builder::new()
                .name("skwir-unmounts".into())
                .spawn(move || {
                    crate::bus::keep(crate::bus::session, &stop, |bus| {
                        let rules = signal_rules(
                            None,
                            &[
                                ("org.gtk.Private.RemoteVolumeMonitor", "MountPreUnmount"),
                                ("org.kde.Solid.Device", "teardownRequested"),
                            ],
                        );
                        crate::bus::listen(
                            bus,
                            rules,
                            &stop,
                            || {},
                            |message| {
                                let points = announced(message);
                                tracing::debug!(?points, "an unmount announced");
                                if !points.is_empty() {
                                    told(UnmountNotice::Announced(points));
                                }
                            },
                        );
                    });
                })
                .is_ok()
        };
        let system = {
            let (stop, told) = (stop.clone(), std::sync::Arc::clone(&told));
            std::thread::Builder::new()
                .name("skwir-udisks-jobs".into())
                .spawn(move || {
                    crate::bus::keep(zbus::blocking::Connection::system, &stop, |bus| {
                        let rules = signal_rules(
                            Some("org.freedesktop.UDisks2"),
                            &[
                                ("org.freedesktop.DBus.ObjectManager", "InterfacesAdded"),
                                ("org.freedesktop.UDisks2.Job", "Completed"),
                            ],
                        );
                        // The unmount jobs begun, by their object, with the
                        // points they unmount.
                        let mut jobs: std::collections::HashMap<String, Vec<PathBuf>> =
                            std::collections::HashMap::new();
                        crate::bus::listen(
                            bus,
                            rules,
                            &stop,
                            || {},
                            |message| match udisks_job(message) {
                                Some(Job::Begun(job, blocks)) => {
                                    let points: Vec<PathBuf> =
                                        blocks.iter().flat_map(|b| points_of_block(b)).collect();
                                    tracing::debug!(?points, "an unmount begun by udisks");
                                    if !points.is_empty() {
                                        jobs.insert(job, points.clone());
                                        told(UnmountNotice::Begun(points));
                                    }
                                }
                                Some(Job::Ended(job, success)) => {
                                    if let Some(points) = jobs.remove(&job)
                                        && !success
                                    {
                                        told(UnmountNotice::Refused(points));
                                    }
                                }
                                None => {}
                            },
                        );
                    });
                })
                .is_ok()
        };
        session || system
    }
    #[cfg(windows)]
    {
        crate::devices::follow(stop, std::sync::Arc::new(told))
    }
    #[cfg(not(any(target_os = "linux", windows)))]
    {
        let _ = (stop, told);
        false
    }
}

/// Match rules for the signals `pairs` names, by interface and member,
/// sent by `sender` where one is named.
#[cfg(target_os = "linux")]
fn signal_rules(
    sender: Option<&'static str>,
    pairs: &[(&'static str, &'static str)],
) -> Vec<zbus::MatchRule<'static>> {
    pairs
        .iter()
        .filter_map(|(interface, member)| {
            let rule = zbus::MatchRule::builder().msg_type(zbus::message::Type::Signal);
            let rule = match sender {
                Some(sender) => rule.sender(sender).ok()?,
                None => rule,
            };
            rule.interface(*interface)
                .and_then(|rule| rule.member(*member))
                .map(zbus::match_rule::Builder::build)
                .ok()
        })
        .collect()
}

/// What a signal of udisks says of an unmount job.
#[cfg(target_os = "linux")]
#[derive(Debug, PartialEq, Eq)]
enum Job {
    /// Begun: its object, and the block devices it unmounts (`loop0`).
    Begun(String, Vec<String>),
    /// Ended: its object, and whether it unmounted.
    Ended(String, bool),
}

/// The unmount job a signal of udisks tells of: one added to its objects
/// (`InterfacesAdded`, a `Job` whose `Operation` is `filesystem-unmount`),
/// or one completed (`Completed`, on the job's object).
#[cfg(target_os = "linux")]
fn udisks_job(message: &zbus::Message) -> Option<Job> {
    use zbus::zvariant::{OwnedObjectPath, OwnedValue};
    let header = message.header();
    match header.member()?.as_str() {
        "InterfacesAdded" => {
            let (job, interfaces) = message
                .body()
                .deserialize::<(
                    OwnedObjectPath,
                    std::collections::HashMap<
                        String,
                        std::collections::HashMap<String, OwnedValue>,
                    >,
                )>()
                .ok()?;
            let props = interfaces.get("org.freedesktop.UDisks2.Job")?;
            let operation = String::try_from(props.get("Operation")?.try_clone().ok()?).ok()?;
            if operation != "filesystem-unmount" {
                return None;
            }
            let objects =
                Vec::<OwnedObjectPath>::try_from(props.get("Objects")?.try_clone().ok()?).ok()?;
            let blocks = objects
                .iter()
                .filter_map(|object| {
                    let name = object
                        .as_str()
                        .strip_prefix("/org/freedesktop/UDisks2/block_devices/")?;
                    unescaped(name).filter(|name| !name.is_empty() && !name.contains('/'))
                })
                .collect();
            Some(Job::Begun(job.as_str().to_owned(), blocks))
        }
        "Completed" => {
            let (success, _) = message.body().deserialize::<(bool, String)>().ok()?;
            Some(Job::Ended(header.path()?.as_str().to_owned(), success))
        }
        _ => None,
    }
}

/// `text` with each `_` and the two hex digits after it made the byte they
/// name: how Solid escapes a udi in an object path, and udisks a device's
/// name (`dm_2d3` for `dm-3`). `None` for an escape cut short.
#[cfg(target_os = "linux")]
fn unescaped(text: &str) -> Option<String> {
    let bytes = text.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut at = 0;
    while at < bytes.len() {
        if bytes[at] == b'_' {
            let hex = text.get(at + 1..at + 3)?;
            out.push(u8::from_str_radix(hex, 16).ok()?);
            at += 3;
        } else {
            out.push(bytes[at]);
            at += 1;
        }
    }
    String::from_utf8(out).ok()
}

/// The mount points a notice of an unmount names: GIO's mount's root,
/// a `file://` URI; Solid's device, by the mounts of its block device or
/// of what it holds unlocked.
#[cfg(target_os = "linux")]
fn announced(message: &zbus::Message) -> Vec<PathBuf> {
    type Mount = (
        String,
        String,
        String,
        String,
        String,
        String,
        bool,
        String,
        Vec<String>,
        String,
        std::collections::HashMap<String, zbus::zvariant::OwnedValue>,
    );
    let header = message.header();
    match header.member().map(zbus::names::MemberName::as_str) {
        Some("MountPreUnmount") => message
            .body()
            .deserialize::<(String, String, Mount)>()
            .ok()
            .and_then(|(_, _, mount)| crate::uri::local_path_of(&mount.5))
            .into_iter()
            .collect(),
        Some("teardownRequested") => header
            .path()
            .and_then(|path| solid_block(path.as_str()))
            .map(|block| points_of_block(&block))
            .unwrap_or_default(),
        _ => Vec::new(),
    }
}

/// The block device (`sdb1`, `dm-3`) Solid's object path for a device
/// names: `/org/kde/solid/Device_` and its udi, `_` escaping each byte
/// other than a letter, a digit and `.~-`; a udisks udi
/// (`/org/freedesktop/UDisks2/block_devices/sdb1`) ends with the device's
/// name, escaped as udisks escapes it (`dm_2d3`).
#[cfg(target_os = "linux")]
fn solid_block(path: &str) -> Option<String> {
    let udi = unescaped(path.strip_prefix("/org/kde/solid/Device_")?)?;
    let name = udi.strip_prefix("/org/freedesktop/UDisks2/block_devices/")?;
    unescaped(name).filter(|name| !name.is_empty() && !name.contains('/'))
}

/// Where `block`'s filesystem is mounted, or the filesystem of what is
/// made of it (a LUKS container's, a volume group's on it): sysfs's
/// `slaves` and the mount table, both in memory.
#[cfg(target_os = "linux")]
fn points_of_block(block: &str) -> Vec<PathBuf> {
    points_made_of(Path::new("/sys/class/block"), block)
}

/// [`points_of_block`], with sysfs's block devices at `sys`.
#[cfg(target_os = "linux")]
fn points_made_of(sys: &Path, block: &str) -> Vec<PathBuf> {
    let Ok(text) = std::fs::read("/proc/self/mountinfo") else {
        return Vec::new();
    };
    skwir_core::fs::volume::mountinfo(&text)
        .into_iter()
        .filter(|mounted| {
            // A device's alone: no other source is a path to follow.
            if !mounted.source.as_encoded_bytes().starts_with(b"/dev/") {
                return false;
            }
            let Some(name) = std::fs::canonicalize(&mounted.source)
                .ok()
                .and_then(|device| Some(device.strip_prefix("/dev").ok()?.to_path_buf()))
            else {
                return false;
            };
            made_of(sys, name.as_os_str(), block, LAYERS)
        })
        .map(|mounted| mounted.point)
        .collect()
}

/// How many layers a block device is followed down through: LVM on LUKS on
/// RAID is three.
#[cfg(target_os = "linux")]
const LAYERS: u8 = 8;

/// Whether the block device `name` is `block`, or is made of it through at
/// most `layers` layers (device mapper's LUKS and LVM, RAID): each one's
/// `slaves` in sysfs at `sys`.
#[cfg(target_os = "linux")]
fn made_of(sys: &Path, name: &std::ffi::OsStr, block: &str, layers: u8) -> bool {
    if name == block {
        return true;
    }
    let Some(below) = layers.checked_sub(1) else {
        return false;
    };
    std::fs::read_dir(sys.join(name).join("slaves")).is_ok_and(|slaves| {
        slaves
            .flatten()
            .any(|slave| made_of(sys, &slave.file_name(), block, below))
    })
}

/// Unmount, then lock the container under it if there is one, so the volume
/// is safe to unplug and asks for its passphrase next time. Blocks: worker only.
pub fn unmount(source: &str) -> anyhow::Result<()> {
    let container = lsblk().and_then(|json| container_of(&json, source));
    udisks("unmount", source, &[], None)?;
    if let Some(container) = container {
        udisks("lock", &container, &[], None)?;
    }
    Ok(())
}

pub fn home() -> Option<PathBuf> {
    skwir_core::os_path::home()
}

/// The user's folders, existing ones only, the home folder itself
/// excluded: XDG's from `~/.config/user-dirs.dirs` on Linux; elsewhere
/// the system's own (Windows' known folders, a Mac's `~/Documents`…),
/// named by XDG's kinds.
pub fn user_dirs() -> Vec<UserDir> {
    let Some(home) = home() else {
        return Vec::new();
    };
    #[cfg(target_os = "linux")]
    let dirs = {
        let Some(cfg) = crate::xdg::config_home() else {
            return Vec::new();
        };
        let text = std::fs::read_to_string(cfg.join("user-dirs.dirs")).unwrap_or_default();
        parse_user_dirs(&text, &home)
    };
    #[cfg(not(target_os = "linux"))]
    let dirs = system_user_dirs();
    dirs.into_iter()
        .filter(|d| d.path != home && d.path.is_dir())
        .collect()
}

/// The system's own user folders, by XDG's kind for each.
#[cfg(not(target_os = "linux"))]
fn system_user_dirs() -> Vec<UserDir> {
    [
        ("desktop", dirs::desktop_dir()),
        ("download", dirs::download_dir()),
        ("documents", dirs::document_dir()),
        ("music", dirs::audio_dir()),
        ("pictures", dirs::picture_dir()),
        ("videos", dirs::video_dir()),
        ("publicshare", dirs::public_dir()),
        ("templates", dirs::template_dir()),
    ]
    .into_iter()
    .filter_map(|(kind, path)| {
        let path = path?;
        let name = path
            .file_name()
            .map_or_else(|| kind.to_owned(), |n| n.to_string_lossy().into_owned());
        Some(UserDir {
            name,
            path,
            kind: kind.to_owned(),
        })
    })
    .collect()
}

/// Parse the `user-dirs.dirs` format.
pub fn parse_user_dirs(text: &str, home: &Path) -> Vec<UserDir> {
    text.lines()
        .map(str::trim)
        .filter(|l| !l.is_empty() && !l.starts_with('#'))
        .filter_map(|l| {
            let (k, v) = l.split_once('=')?;
            let kind = k
                .strip_prefix("XDG_")?
                .strip_suffix("_DIR")?
                .to_ascii_lowercase();
            let v = v.trim().trim_matches('"');
            let path = if let Some(rest) = v.strip_prefix("$HOME/") {
                home.join(rest)
            } else if v == "$HOME" {
                home.to_path_buf()
            } else {
                PathBuf::from(v)
            };
            let name = path
                .file_name()
                .map_or_else(|| kind.clone(), |n| n.to_string_lossy().into_owned());
            Some(UserDir { name, path, kind })
        })
        .collect()
}

/// GTK bookmarks from `~/.config/gtk-3.0/bookmarks`.
pub fn bookmarks() -> Vec<Bookmark> {
    let Some(file) = bookmarks_file() else {
        return Vec::new();
    };
    let text = std::fs::read_to_string(file).unwrap_or_default();
    parse_bookmarks(&text)
}

/// Parse `file:///path%20x Label` lines. Non-`file://` entries are skipped.
pub fn parse_bookmarks(text: &str) -> Vec<Bookmark> {
    lines_of(text).iter().filter_map(folder_bookmark).collect()
}

fn folder_bookmark(line: &Line) -> Option<Bookmark> {
    let (path, _, label) = line.mark.clone()?;
    let title = label
        .or_else(|| path.file_name().map(|n| n.to_string_lossy().into_owned()))
        .unwrap_or_else(|| path.display().to_string());
    Some(Bookmark { title, path })
}

/// A bookmark of a remote folder: its address as written
/// (`sftp://me@example.com/home/me`, `smb://nas/photos`) and its label, the
/// address's last part when it has none.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RemoteBookmark {
    pub title: String,
    pub address: String,
}

/// The GTK bookmarks of remote folders, which another file manager wrote
/// (ADR-077): what the remote plugin opens.
pub fn remote_bookmarks() -> Vec<RemoteBookmark> {
    let Some(file) = bookmarks_file() else {
        return Vec::new();
    };
    parse_remote_bookmarks(&std::fs::read_to_string(file).unwrap_or_default())
}

/// The lines of `text` that bookmark an address of another scheme than
/// `file://` (`sftp://host/path Label`).
pub fn parse_remote_bookmarks(text: &str) -> Vec<RemoteBookmark> {
    lines_of(text).iter().filter_map(remote_bookmark).collect()
}

fn remote_bookmark(line: &Line) -> Option<RemoteBookmark> {
    if line.mark.is_some() {
        return None;
    }
    let (address, label) = address_and_label(line);
    let (scheme, rest) = address.split_once("://")?;
    if scheme.is_empty() || scheme.eq_ignore_ascii_case("file") || rest.is_empty() {
        return None;
    }
    let title = label.map_or_else(|| address_name(address), str::to_owned);
    Some(RemoteBookmark {
        title,
        address: address.to_owned(),
    })
}

/// The name an address shows without a label: its last part, decoded.
pub fn address_name(address: &str) -> String {
    let rest = address.split_once("://").map_or(address, |(_, rest)| rest);
    let named = rest.trim_end_matches('/');
    let last = named.rsplit('/').next().unwrap_or(named);
    String::from_utf8_lossy(&crate::uri::percent_decode(last)).into_owned()
}

/// A bookmark of either kind, as the file orders them: what the sidebar
/// lists, a folder's and an address's side by side.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Bookmarked {
    Folder(Bookmark),
    Address(RemoteBookmark),
}

/// Every GTK bookmark, folders and addresses, in the file's order.
pub fn every_bookmark() -> Vec<Bookmarked> {
    let Some(file) = bookmarks_file() else {
        return Vec::new();
    };
    parse_every_bookmark(&std::fs::read_to_string(file).unwrap_or_default())
}

/// The bookmarks of `text`, folders and addresses, in its order.
pub fn parse_every_bookmark(text: &str) -> Vec<Bookmarked> {
    lines_of(text)
        .iter()
        .filter_map(|line| {
            folder_bookmark(line)
                .map(Bookmarked::Folder)
                .or_else(|| remote_bookmark(line).map(Bookmarked::Address))
        })
        .collect()
}

/// What a bookmark keeps: a folder, or a remote folder by its address
/// (`sftp://me@host/docs`, as the file writes it). Edits name either, so
/// the two kinds share one order.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Marked {
    Folder(PathBuf),
    Address(String),
}

impl Marked {
    pub fn path(&self) -> Option<&Path> {
        match self {
            Self::Folder(path) => Some(path),
            Self::Address(_) => None,
        }
    }

    pub fn address(&self) -> Option<&str> {
        match self {
            Self::Folder(_) => None,
            Self::Address(address) => Some(address),
        }
    }
}

/// A change to the bookmarks file, as data: what a command applies, and
/// what [`edit_text`] hands back to take it back.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum BookmarkEdit {
    /// Bookmark `path`, labelled `title` (the folder's own name when
    /// `None`), before the bookmark of `before` (last when `None` or when
    /// that one is gone).
    Add {
        path: PathBuf,
        title: Option<String>,
        before: Option<Marked>,
    },
    Remove {
        path: PathBuf,
    },
    /// Label a bookmark `title`; `None` is the folder's own name (an
    /// address's last part).
    Rename {
        mark: Marked,
        title: Option<String>,
    },
    /// Put a bookmark before `before`'s, last when `None`: folders and
    /// addresses in one order.
    Move {
        mark: Marked,
        before: Option<Marked>,
    },
    /// Bookmark a remote folder by its address (`sftp://me@host/docs`, as
    /// `uri::address_uri` writes it), labelled `title`, before `before`'s
    /// bookmark (last when `None`): what other file managers write, and
    /// what opens it again once its mount is gone (ADR-077).
    AddAddress {
        address: String,
        title: Option<String>,
        before: Option<Marked>,
    },
    /// Remove the bookmark of `address`.
    RemoveAddress {
        address: String,
    },
}

impl BookmarkEdit {
    /// The folder the edit is about; `None` for an address's.
    pub fn path(&self) -> Option<&Path> {
        match self {
            Self::Add { path, .. } | Self::Remove { path } => Some(path),
            Self::Rename { mark, .. } | Self::Move { mark, .. } => mark.path(),
            Self::AddAddress { .. } | Self::RemoveAddress { .. } => None,
        }
    }

    /// The address the edit is about; `None` for a folder's.
    pub fn address(&self) -> Option<&str> {
        match self {
            Self::AddAddress { address, .. } | Self::RemoveAddress { address } => Some(address),
            Self::Rename { mark, .. } | Self::Move { mark, .. } => mark.address(),
            Self::Add { .. } | Self::Remove { .. } => None,
        }
    }
}

/// The address a line bookmarks, when it is no folder's.
fn address_of(line: &Line) -> Option<&str> {
    if line.mark.is_some() {
        return None;
    }
    let address = line.raw.trim().split(' ').next()?;
    address.contains("://").then_some(address)
}

/// An address line's address and its label, when it has one.
fn address_and_label(line: &Line) -> (&str, Option<&str>) {
    let trimmed = line.raw.trim();
    match trimmed.split_once(' ') {
        Some((address, label)) => (address, Some(label.trim()).filter(|l| !l.is_empty())),
        None => (trimmed, None),
    }
}

/// What a line bookmarks, when it bookmarks something.
fn marked(line: &Line) -> Option<Marked> {
    match &line.mark {
        Some((path, ..)) => Some(Marked::Folder(path.clone())),
        None => address_of(line).map(|address| Marked::Address(address.to_owned())),
    }
}

fn is(line: &Line, mark: &Marked) -> bool {
    match mark {
        Marked::Folder(path) => line.mark.as_ref().is_some_and(|(p, ..)| p == path),
        Marked::Address(address) => address_of(line) == Some(address.as_str()),
    }
}

/// A line of the bookmarks file. Lines that are not folders (`sftp://`
/// bookmarks, blank lines) are kept as they were written.
struct Line {
    raw: String,
    /// The folder, its URI as written, and its label.
    mark: Option<(PathBuf, String, Option<String>)>,
}

fn lines_of(text: &str) -> Vec<Line> {
    text.lines()
        .map(|raw| {
            let trimmed = raw.trim();
            let (uri, label) = match trimmed.split_once(' ') {
                Some((uri, label)) => (uri, Some(label.trim()).filter(|l| !l.is_empty())),
                None => (trimmed, None),
            };
            let mark = crate::uri::path_of(uri)
                .map(|path| (path, uri.to_owned(), label.map(str::to_owned)));
            Line {
                raw: raw.to_owned(),
                mark,
            }
        })
        .collect()
}

fn text_of(lines: &[Line]) -> String {
    lines.iter().fold(String::new(), |mut out, line| {
        out.push_str(&line.raw);
        out.push('\n');
        out
    })
}

fn mark_line(uri: &str, label: Option<&str>) -> String {
    label.map_or_else(|| uri.to_owned(), |label| format!("{uri} {label}"))
}

/// A label as the file stores it: one line, and none at all when it is
/// empty or the folder's own name, so the bookmark follows a rename.
fn label_for(path: &Path, title: Option<&str>) -> Option<String> {
    let title = title?
        .split(['\n', '\r'])
        .collect::<Vec<_>>()
        .join(" ")
        .trim()
        .to_owned();
    let own = path.file_name().map(|n| n.to_string_lossy());
    (!title.is_empty() && own.as_deref() != Some(title.as_str())).then_some(title)
}

/// An address's label as the file stores it: one line, none when empty.
/// Kept as given otherwise: an address has no name of its own to follow.
fn address_label(title: Option<&str>) -> Option<String> {
    title
        .map(|t| {
            t.split(['\n', '\r'])
                .collect::<Vec<_>>()
                .join(" ")
                .trim()
                .to_owned()
        })
        .filter(|t| !t.is_empty())
}

fn position(lines: &[Line], mark: &Marked) -> Option<usize> {
    lines.iter().position(|l| is(l, mark))
}

/// The bookmark after line `index`, folder or address, other than `mark`
/// itself.
fn next_after(lines: &[Line], index: usize, mark: &Marked) -> Option<Marked> {
    lines[index + 1..]
        .iter()
        .filter(|l| !is(l, mark))
        .find_map(marked)
}

/// Where a line goes to stand before `before`'s bookmark: at its line, or
/// at the end.
fn insertion(lines: &[Line], before: Option<&Marked>) -> usize {
    before
        .and_then(|before| position(lines, before))
        .unwrap_or(lines.len())
}

/// Apply `edit` to the text of a bookmarks file. `None` when it changes
/// nothing (the folder is already there, or is not, or already has that
/// label or place), else the new text and the edit that reverts it. Other
/// lines, and the URIs other applications wrote, are kept as they were.
pub fn edit_text(text: &str, edit: &BookmarkEdit) -> Option<(String, BookmarkEdit)> {
    let mut lines = lines_of(text);
    let inverse = match edit {
        BookmarkEdit::Add {
            path,
            title,
            before,
        } => {
            if position(&lines, &Marked::Folder(path.clone())).is_some() {
                return None;
            }
            let uri = crate::uri::file_uri(path);
            let label = label_for(path, title.as_deref());
            let at = insertion(&lines, before.as_ref());
            lines.insert(
                at,
                Line {
                    raw: mark_line(&uri, label.as_deref()),
                    mark: Some((path.clone(), uri, label)),
                },
            );
            BookmarkEdit::Remove { path: path.clone() }
        }
        BookmarkEdit::Remove { path } => {
            let mark = Marked::Folder(path.clone());
            let first = position(&lines, &mark)?;
            let before = next_after(&lines, first, &mark);
            let title = lines[first].mark.as_ref().and_then(|(.., l)| l.clone());
            lines.retain(|l| !is(l, &mark));
            BookmarkEdit::Add {
                path: path.clone(),
                title,
                before,
            }
        }
        BookmarkEdit::Rename { mark, title } => {
            let at = position(&lines, mark)?;
            let line = &mut lines[at];
            let old = match mark {
                Marked::Folder(path) => {
                    let label = label_for(path, title.as_deref());
                    let (_, uri, old) = line.mark.as_mut()?;
                    if *old == label {
                        return None;
                    }
                    line.raw = mark_line(uri, label.as_deref());
                    std::mem::replace(old, label)
                }
                Marked::Address(_) => {
                    let label = address_label(title.as_deref());
                    let (address, old) = address_and_label(line);
                    if old == label.as_deref() {
                        return None;
                    }
                    let (address, old) = (address.to_owned(), old.map(str::to_owned));
                    line.raw = mark_line(&address, label.as_deref());
                    old
                }
            };
            BookmarkEdit::Rename {
                mark: mark.clone(),
                title: old,
            }
        }
        BookmarkEdit::AddAddress {
            address,
            title,
            before,
        } => {
            if position(&lines, &Marked::Address(address.clone())).is_some() {
                return None;
            }
            let label = address_label(title.as_deref());
            let at = insertion(&lines, before.as_ref());
            lines.insert(
                at,
                Line {
                    raw: mark_line(address, label.as_deref()),
                    mark: None,
                },
            );
            BookmarkEdit::RemoveAddress {
                address: address.clone(),
            }
        }
        BookmarkEdit::RemoveAddress { address } => {
            let mark = Marked::Address(address.clone());
            let at = position(&lines, &mark)?;
            let before = next_after(&lines, at, &mark);
            let title = address_and_label(&lines[at]).1.map(str::to_owned);
            lines.retain(|l| !is(l, &mark));
            BookmarkEdit::AddAddress {
                address: address.clone(),
                title,
                before,
            }
        }
        BookmarkEdit::Move { mark, before } => {
            let from = position(&lines, mark)?;
            let was = next_after(&lines, from, mark);
            if before.as_ref() == Some(mark) || was == *before {
                return None;
            }
            let line = lines.remove(from);
            let at = insertion(&lines, before.as_ref());
            lines.insert(at, line);
            BookmarkEdit::Move {
                mark: mark.clone(),
                before: was,
            }
        }
    };
    Some((text_of(&lines), inverse))
}

/// The GTK bookmarks file every file manager and file dialog on the desktop
/// shares, as it is named (it may be a link).
pub fn bookmarks_file() -> Option<PathBuf> {
    Some(crate::xdg::config_home()?.join("gtk-3.0").join("bookmarks"))
}

/// The folders to watch to see the bookmarks change: the file's own, and
/// the one it points into when it is a link (a dotfile manager's), since
/// writers replace the file rather than write into it.
pub fn bookmarks_folders() -> Vec<PathBuf> {
    let Some(file) = bookmarks_file() else {
        return Vec::new();
    };
    let mut folders: Vec<PathBuf> = file.parent().map(Path::to_path_buf).into_iter().collect();
    if let Some(target) = skwir_core::os_path::canonical(&file)
        .ok()
        .and_then(|target| target.parent().map(Path::to_path_buf))
        && !folders.contains(&target)
    {
        folders.push(target);
    }
    folders
}

/// Apply `edit` to the GTK bookmarks. Returns the edit that reverts it, or
/// `None` when nothing changed. Writes a file: call from a worker. A link
/// is followed and a read-only file refused ([`crate::file::rewrite_words`]).
pub fn edit_bookmarks(edit: &BookmarkEdit) -> anyhow::Result<Option<BookmarkEdit>> {
    let link = bookmarks_file().ok_or_else(|| {
        anyhow::Error::from(words!(
            places_no_config_dir = "no configuration directory",
            note = "Why a bookmark cannot be kept: the user has no configuration folder to write it in."
        ))
    })?;
    let read_only = words!(
        places_bookmarks_read_only =
            "the bookmarks file is read-only; change it where it is managed",
        note = "Why a bookmark was not kept: the desktop's bookmarks file is a link to a file that cannot be written, as a configuration managed elsewhere (Nix's Home Manager) makes it."
    );
    crate::file::rewrite_words(&link, read_only, |text| edit_text(text, edit))
}

/// Mounted volumes the user cares about.
pub fn mounts() -> Vec<Mount> {
    imp::mounts()
}

/// Filesystems that hold user data (others are pseudo or system).
fn is_user_fs(fstype: &str) -> bool {
    matches!(
        fstype,
        "btrfs"
            | "ext2"
            | "ext3"
            | "ext4"
            | "xfs"
            | "vfat"
            | "exfat"
            | "ntfs"
            | "ntfs3"
            | "f2fs"
            | "zfs"
            | "hfsplus"
            | "apfs"
            | "udf"
            | "iso9660"
    ) || fstype.starts_with("fuse")
}

fn is_interesting_mountpoint(p: &Path) -> bool {
    p == Path::new("/")
        || p == Path::new("/home")
        || p == Path::new("/boot")
        || p.starts_with("/run/media")
        || p.starts_with("/media")
        || p.starts_with("/mnt")
}

/// The volumes worth showing among `/proc/self/mountinfo`'s mounts
/// (`fs::volume::mountinfo`, the one reading of it).
pub fn parse_mountinfo(text: &(impl AsRef<[u8]> + ?Sized)) -> Vec<Mount> {
    let mut out: Vec<Mount> = Vec::new();
    for mounted in skwir_core::fs::volume::mountinfo(text) {
        let mountpoint = mounted.point;
        if !is_user_fs(&mounted.fstype) || !is_interesting_mountpoint(&mountpoint) {
            continue;
        }
        if out.iter().any(|m| m.path == mountpoint) {
            continue;
        }
        let label = if mountpoint == Path::new("/") {
            words!(
                places_root = "Root",
                noun,
                note = "The name of the volume the system runs from (/), in the sidebar's Devices."
            )
        } else {
            Words::as_written(
                mountpoint
                    .file_name()
                    .map(|n| n.to_string_lossy().into_owned())
                    .unwrap_or_default(),
            )
        };
        let udisks = skwir_core::fs::volume::user_mounted(&mountpoint);
        out.push(Mount {
            label,
            path: mountpoint,
            fstype: mounted.fstype,
            udisks,
            removable: false,
            source: mounted.source.to_string_lossy().into_owned(),
            root: mounted.root,
            locked: false,
        });
    }
    out
}

#[cfg(target_os = "linux")]
mod imp {
    use super::{Mount, parse_mountinfo};

    /// Reads procfs, `/dev` and sysfs, never a volume itself.
    pub fn mounts() -> Vec<Mount> {
        let mut mounts = std::fs::read("/proc/self/mountinfo")
            .map(|t| parse_mountinfo(&t))
            .unwrap_or_default();
        for mount in &mut mounts {
            mount.removable =
                skwir_core::fs::volume::device_removable(std::path::Path::new(&mount.source));
        }
        mounts
    }
}

/// Windows: the local drives, each its label (or what Windows calls an
/// unnamed one) with its letter, removable when it is plugged in, locked
/// when BitLocker keeps it so (asked only of a drive that says nothing of
/// itself: an empty card reader's says nothing either).
#[cfg(windows)]
mod imp {
    use super::Mount;

    pub fn mounts() -> Vec<Mount> {
        crate::windows::local_drives()
            .into_iter()
            .map(|root| {
                let letter = root.to_string_lossy().trim_end_matches('\\').to_owned();
                let information = crate::windows::volume_information(&root);
                let locked = information.is_none() && crate::windows::drive_locked(&root);
                let (label, fstype) = information.unwrap_or_default();
                let label = if label.is_empty() {
                    skwir_core::words!(
                        places_local_disk = "Local Disk ({ $letter })",
                        letter = letter.clone(),
                        note = "A Windows drive that has no name of its own, as Windows names it. letter: its drive letter (C:)."
                    )
                } else {
                    skwir_core::words!(
                        places_drive = "{ $label } ({ $letter })",
                        label = label,
                        letter = letter.clone(),
                        note = "A Windows drive, as Windows names it. label: the drive's own name; letter: its drive letter (C:)."
                    )
                };
                Mount {
                    label,
                    removable: crate::windows::is_removable_drive(&root),
                    fstype,
                    udisks: false,
                    source: letter,
                    root: std::path::PathBuf::from("\\"),
                    path: root,
                    locked,
                }
            })
            .collect()
    }
}

/// macOS: what `getmntinfo` says is mounted, the root and the volumes
/// under `/Volumes` the Finder shows (not those it hides). Compiled,
/// never run (ADR-035).
#[cfg(target_os = "macos")]
mod imp {
    use std::ffi::CStr;
    use std::path::{Path, PathBuf};

    use super::Mount;

    pub fn mounts() -> Vec<Mount> {
        let mut list: *mut libc::statfs = std::ptr::null_mut();
        // SAFETY: an out pointer the system fills with its own array of
        // `count` entries, which it keeps (it is not to be freed).
        #[allow(unsafe_code)]
        let count = unsafe { libc::getmntinfo(&raw mut list, libc::MNT_NOWAIT) };
        let Ok(count) = usize::try_from(count) else {
            return Vec::new();
        };
        if list.is_null() {
            return Vec::new();
        }
        // SAFETY: `count` entries at `list`, alive until the next call on
        // this thread; read now.
        #[allow(unsafe_code)]
        let entries = unsafe { std::slice::from_raw_parts(list, count) };
        let text = |field: &[libc::c_char]| {
            // SAFETY: the system's fixed fields are NUL-terminated.
            #[allow(unsafe_code)]
            let text = unsafe { CStr::from_ptr(field.as_ptr()) };
            text.to_string_lossy().into_owned()
        };
        entries
            .iter()
            .filter(|e| e.f_flags & (libc::MNT_DONTBROWSE as u32) == 0)
            .filter_map(|e| {
                let path = PathBuf::from(text(&e.f_mntonname));
                if path != Path::new("/") && !path.starts_with("/Volumes") {
                    return None;
                }
                // The Finder's name of the system's volume, as it writes it.
                let label = skwir_core::words::Words::as_written(if path == Path::new("/") {
                    "Macintosh HD".to_owned()
                } else {
                    path.file_name()?.to_string_lossy().into_owned()
                });
                Some(Mount {
                    label,
                    fstype: text(&e.f_fstypename),
                    udisks: false,
                    removable: false,
                    source: text(&e.f_mntfromname),
                    root: PathBuf::from("/"),
                    path,
                    locked: false,
                })
            })
            .collect()
    }
}

/// macOS's encrypted APFS volumes through `diskutil` (ADR-088): those
/// locked listed, unlocked with the passphrase on its standard input
/// (`-stdinpassphrase`), mounted where macOS mounts them (`/Volumes`).
/// Compiled, never run; the parser runs in the tests everywhere.
#[cfg(any(target_os = "macos", test))]
mod apfs {
    use super::Volume;

    /// The volumes `diskutil apfs list -plist` says are locked: each by its
    /// device (`disk4s1`), name and UUID. A volume with a role (the system
    /// of another macOS, its data, a recovery) is offered hidden.
    pub fn locked(plist: &[u8]) -> Vec<Volume> {
        let Ok(listing) = plist::Value::from_reader(std::io::Cursor::new(plist)) else {
            return Vec::new();
        };
        let field = |volume: &plist::Dictionary, key: &str| volume.get(key).cloned();
        listing
            .as_dictionary()
            .and_then(|top| top.get("Containers")?.as_array())
            .into_iter()
            .flatten()
            .filter_map(|container| container.as_dictionary()?.get("Volumes")?.as_array())
            .flatten()
            .filter_map(plist::Value::as_dictionary)
            .filter(|volume| field(volume, "Locked").and_then(|v| v.as_boolean()) == Some(true))
            .filter_map(|volume| {
                let device = field(volume, "DeviceIdentifier")?.into_string()?;
                let roles = field(volume, "Roles")
                    .and_then(plist::Value::into_array)
                    .unwrap_or_default();
                Some(Volume {
                    device,
                    label: field(volume, "Name")
                        .and_then(plist::Value::into_string)
                        .unwrap_or_default(),
                    size: field(volume, "CapacityInUse")
                        .and_then(|v| v.as_unsigned_integer())
                        .unwrap_or(0),
                    locked: true,
                    hotplug: false,
                    container: None,
                    system: !roles.is_empty(),
                    uuid: field(volume, "APFSVolumeUUID").and_then(plist::Value::into_string),
                })
            })
            .collect()
    }

    /// Where `diskutil info -plist` says a volume is mounted, `None` when
    /// it is not (an empty `MountPoint`).
    pub fn mount_point(plist: &[u8]) -> Option<std::path::PathBuf> {
        let info = plist::Value::from_reader(std::io::Cursor::new(plist)).ok()?;
        let at = info.as_dictionary()?.get("MountPoint")?.as_string()?;
        (!at.is_empty()).then(|| std::path::PathBuf::from(at))
    }

    /// Whether what `diskutil apfs unlockVolume` said is a passphrase
    /// refused ("Passphrase incorrect or user does not exist").
    pub fn refused(said: &str) -> bool {
        said.contains("Passphrase incorrect")
    }

    /// What `diskutil` answered `args`, with `input` on its standard input.
    #[cfg(target_os = "macos")]
    fn diskutil(args: &[&str], input: Option<&[u8]>) -> anyhow::Result<crate::process::Finished> {
        crate::process::run_to_exit(
            crate::process::command("diskutil").args(args),
            input,
            &skwir_core::CancelToken::new(),
            super::UDISKS_TIMEOUT,
        )
        .map_err(|error| {
            skwir_core::words!(
                places_diskutil_unrun = "diskutil could not be run: { $error }",
                error = error,
                note = "diskutil is the program that mounts and unlocks a Mac's volumes (a name). error: why it could not be run."
            )
            .into()
        })
    }

    /// `diskutil apfs list -plist`.
    #[cfg(target_os = "macos")]
    pub fn list() -> Option<Vec<u8>> {
        let finished = diskutil(&["apfs", "list", "-plist"], None).ok()?;
        finished.status.success().then_some(finished.stdout)
    }

    #[cfg(target_os = "macos")]
    pub fn unlock(device: &str, passphrase: &str) -> anyhow::Result<String> {
        // A line, as macOS's own scripts give it.
        let mut input = passphrase.as_bytes().to_vec();
        input.push(b'\n');
        let finished = diskutil(
            &["apfs", "unlockVolume", device, "-stdinpassphrase"],
            Some(&input),
        )?;
        if finished.status.success() {
            return Ok(device.to_owned());
        }
        let said = format!(
            "{}{}",
            String::from_utf8_lossy(&finished.stdout),
            finished.stderr
        );
        if refused(&said) {
            return Err(anyhow::Error::new(super::Refused));
        }
        // Unlocked meanwhile (macOS's own dialog, a Keychain): mounted.
        if said.contains("already unlocked") {
            return Ok(device.to_owned());
        }
        anyhow::bail!("{}", said.trim())
    }

    /// Mounted where macOS mounts it: as it was unlocked, else now.
    #[cfg(target_os = "macos")]
    pub fn mount(device: &str) -> anyhow::Result<std::path::PathBuf> {
        let at = || -> anyhow::Result<Option<std::path::PathBuf>> {
            let finished = diskutil(&["info", "-plist", device], None)?;
            anyhow::ensure!(finished.status.success(), "{}", finished.stderr.trim());
            Ok(mount_point(&finished.stdout))
        };
        if let Some(path) = at()? {
            return Ok(path);
        }
        let finished = diskutil(&["mount", device], None)?;
        anyhow::ensure!(finished.status.success(), "{}", finished.stderr.trim());
        at()?.ok_or_else(|| {
            skwir_core::words!(
                places_diskutil_no_mount_point = "diskutil did not say where it mounted the volume",
                note = "diskutil is the program that mounts a Mac's volumes (a name)."
            )
            .into()
        })
    }
}

#[cfg(test)]
mod tests {

    /// GIO's notice of an unmount names the mount by its root, a `file://`
    /// URI: its folder is told, whatever bytes it holds; a server's (an
    /// `sftp://` root) is none of a drive's.
    #[cfg(target_os = "linux")]
    #[test]
    fn an_unmount_gio_announces_names_its_folder() {
        let notice = |root: &str| {
            let mount = (
                "0x1",
                "USB stick",
                "",
                "",
                "",
                root,
                true,
                "",
                Vec::<String>::new(),
                "",
                std::collections::HashMap::<String, zbus::zvariant::Value>::new(),
            );
            let message = zbus::Message::signal(
                "/org/gtk/Private/RemoteVolumeMonitor",
                "org.gtk.Private.RemoteVolumeMonitor",
                "MountPreUnmount",
            )
            .unwrap()
            .build(&("org.gtk.vfs.UDisks2VolumeMonitor", "0x1", mount))
            .unwrap();
            super::announced(&message)
        };
        assert_eq!(
            notice("file:///run/media/example/USB%20stick%2C%20%C3%A9t%C3%A9"),
            [PathBuf::from("/run/media/example/USB stick, été")]
        );
        assert!(notice("sftp://example.com/").is_empty());
    }

    /// A notice naming a partition (Solid's teardown of a LUKS container)
    /// names the volumes made of it, through every layer between.
    #[cfg(target_os = "linux")]
    #[test]
    fn a_block_device_is_followed_down_through_its_layers() {
        // LVM's volume (dm-5) on a LUKS container's cleartext (dm-4) on a
        // partition (sdb2), as sysfs's `slaves` say.
        let sys = tempfile::tempdir().unwrap();
        for (device, slave) in [
            ("dm-5", Some("dm-4")),
            ("dm-4", Some("sdb2")),
            ("sdb2", None),
        ] {
            let slaves = sys.path().join(device).join("slaves");
            std::fs::create_dir_all(&slaves).unwrap();
            if let Some(slave) = slave {
                std::fs::create_dir_all(slaves.join(slave)).unwrap();
            }
        }
        let made_of = |name: &str, block: &str, layers: u8| {
            super::made_of(sys.path(), name.as_ref(), block, layers)
        };
        assert!(made_of("sdb2", "sdb2", 0), "itself");
        assert!(made_of("dm-4", "sdb2", super::LAYERS), "the container's");
        assert!(made_of("dm-5", "sdb2", super::LAYERS), "the volume group's");
        assert!(!made_of("dm-5", "sdb2", 1), "one layer only");
        assert!(!made_of("dm-5", "sdc1", super::LAYERS), "another drive's");
        assert!(!made_of("nvme0n1", "sdb2", super::LAYERS), "not there");
    }

    /// Solid names the device whose teardown it announces by its udi,
    /// escaped twice: by Solid in its object path, by udisks in its own.
    #[cfg(target_os = "linux")]
    #[test]
    fn a_teardown_solid_announces_names_its_block_device() {
        assert_eq!(
            super::solid_block(
                "/org/kde/solid/Device__2Forg_2Ffreedesktop_2FUDisks2_2Fblock_5Fdevices_2Fsdb1"
            )
            .as_deref(),
            Some("sdb1")
        );
        assert_eq!(
            super::solid_block(
                "/org/kde/solid/Device__2Forg_2Ffreedesktop_2FUDisks2_2Fblock_5Fdevices_2Fdm_5F2d3"
            )
            .as_deref(),
            Some("dm-3")
        );
        // Another backend's device, or a path cut short, names none.
        assert_eq!(
            super::solid_block("/org/kde/solid/Device__2Ffstab_2Fnas"),
            None
        );
        assert_eq!(super::solid_block("/org/kde/solid/Device__2"), None);
        assert_eq!(super::solid_block("/org/kde/other"), None);
    }
    /// udisks's unmount jobs, whoever asked for them (`udisksctl`,
    /// udiskie): begun as their object is added, naming the block devices
    /// they unmount, ended as they complete, refused or not. Its other
    /// jobs say nothing. The fields as `gdbus monitor` showed them.
    #[cfg(target_os = "linux")]
    #[test]
    fn an_unmount_udisks_begins_names_its_devices_and_ends() {
        use super::Job;
        use std::collections::HashMap;
        use zbus::zvariant::{ObjectPath, Value};
        let added = |operation: &str, blocks: &[&str]| {
            let objects: Vec<ObjectPath<'_>> = blocks
                .iter()
                .map(|b| ObjectPath::try_from(*b).unwrap())
                .collect();
            let job: HashMap<&str, Value<'_>> = HashMap::from([
                ("Operation", Value::from(operation)),
                ("Objects", Value::from(objects)),
                ("StartedByUID", Value::from(0u32)),
            ]);
            let message = zbus::Message::signal(
                "/org/freedesktop/UDisks2",
                "org.freedesktop.DBus.ObjectManager",
                "InterfacesAdded",
            )
            .unwrap()
            .build(&(
                ObjectPath::try_from("/org/freedesktop/UDisks2/jobs/74").unwrap(),
                HashMap::from([("org.freedesktop.UDisks2.Job", job)]),
            ))
            .unwrap();
            super::udisks_job(&message)
        };
        assert_eq!(
            added(
                "filesystem-unmount",
                &[
                    "/org/freedesktop/UDisks2/block_devices/loop0",
                    "/org/freedesktop/UDisks2/block_devices/dm_2d3",
                ]
            ),
            Some(Job::Begun(
                "/org/freedesktop/UDisks2/jobs/74".into(),
                vec!["loop0".into(), "dm-3".into()]
            ))
        );
        assert_eq!(
            added(
                "filesystem-mount",
                &["/org/freedesktop/UDisks2/block_devices/loop0"]
            ),
            None
        );
        let completed = |success: bool, why: &str| {
            let message = zbus::Message::signal(
                "/org/freedesktop/UDisks2/jobs/76",
                "org.freedesktop.UDisks2.Job",
                "Completed",
            )
            .unwrap()
            .build(&(success, why))
            .unwrap();
            super::udisks_job(&message)
        };
        assert_eq!(
            completed(false, "target is busy"),
            Some(Job::Ended("/org/freedesktop/UDisks2/jobs/76".into(), false))
        );
        assert_eq!(
            completed(true, ""),
            Some(Job::Ended("/org/freedesktop/UDisks2/jobs/76".into(), true))
        );
    }

    /// What `diskutil apfs list -plist` answers, cut to what is read: a
    /// Mac's own container (its system, its data, unlocked) and a drive's,
    /// one volume locked, one not encrypted. Synthetic names and UUIDs, in
    /// the keys captured from real answers.
    const APFS_LIST: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>Containers</key>
  <array>
    <dict>
      <key>APFSContainerUUID</key><string>00000000-0000-4000-8000-000000000001</string>
      <key>Volumes</key>
      <array>
        <dict>
          <key>APFSVolumeUUID</key><string>00000000-0000-4000-8000-000000000002</string>
          <key>DeviceIdentifier</key><string>disk3s1</string>
          <key>Encryption</key><true/>
          <key>FileVault</key><true/>
          <key>Locked</key><false/>
          <key>Name</key><string>Macintosh HD</string>
          <key>Roles</key><array><string>System</string></array>
        </dict>
        <dict>
          <key>APFSVolumeUUID</key><string>00000000-0000-4000-8000-000000000003</string>
          <key>DeviceIdentifier</key><string>disk3s7</string>
          <key>FileVault</key><true/>
          <key>Locked</key><true/>
          <key>Name</key><string>Other Data</string>
          <key>Roles</key><array><string>Data</string></array>
        </dict>
      </array>
    </dict>
    <dict>
      <key>APFSContainerUUID</key><string>00000000-0000-4000-8000-000000000004</string>
      <key>Volumes</key>
      <array>
        <dict>
          <key>APFSVolumeUUID</key><string>00000000-0000-4000-8000-000000000005</string>
          <key>CapacityInUse</key><integer>1000000000</integer>
          <key>DeviceIdentifier</key><string>disk5s1</string>
          <key>Encryption</key><true/>
          <key>FileVault</key><true/>
          <key>Locked</key><true/>
          <key>Name</key><string>It's a backup</string>
          <key>Roles</key><array/>
        </dict>
        <dict>
          <key>APFSVolumeUUID</key><string>00000000-0000-4000-8000-000000000006</string>
          <key>DeviceIdentifier</key><string>disk5s2</string>
          <key>Encryption</key><false/>
          <key>FileVault</key><false/>
          <key>Locked</key><false/>
          <key>Name</key><string>Open</string>
          <key>Roles</key><array/>
        </dict>
      </array>
    </dict>
  </array>
</dict>
</plist>"#;

    /// A Mac's locked encrypted volumes are listed by their device, name
    /// and UUID; another macOS's (a role) offered hidden; what is unlocked
    /// or not encrypted is not one to unlock.
    #[test]
    fn a_macs_locked_apfs_volumes_are_listed() {
        let volumes = super::apfs::locked(APFS_LIST.as_bytes());
        let listed: Vec<(&str, &str, bool, Option<&str>)> = volumes
            .iter()
            .map(|v| {
                (
                    v.device.as_str(),
                    v.label.as_str(),
                    v.system,
                    v.uuid.as_deref(),
                )
            })
            .collect();
        assert_eq!(
            listed,
            [
                (
                    "disk3s7",
                    "Other Data",
                    true,
                    Some("00000000-0000-4000-8000-000000000003")
                ),
                (
                    "disk5s1",
                    "It's a backup",
                    false,
                    Some("00000000-0000-4000-8000-000000000005")
                ),
            ]
        );
        assert!(volumes.iter().all(|v| v.locked));
        assert_eq!(volumes[1].size, 1_000_000_000);
        assert!(super::apfs::locked(b"not a plist").is_empty());
    }

    /// Where `diskutil info -plist` says a volume is mounted; an empty
    /// mount point is none. A refusal is told by what diskutil says.
    #[test]
    fn diskutil_says_where_a_volume_is_and_when_a_passphrase_is_refused() {
        let info = |at: &str| {
            format!(
                r#"<?xml version="1.0" encoding="UTF-8"?>
<plist version="1.0"><dict><key>DeviceIdentifier</key><string>disk5s1</string><key>MountPoint</key><string>{at}</string></dict></plist>"#
            )
        };
        assert_eq!(
            super::apfs::mount_point(info("/Volumes/It's a backup").as_bytes()),
            Some(std::path::PathBuf::from("/Volumes/It's a backup"))
        );
        assert_eq!(super::apfs::mount_point(info("").as_bytes()), None);
        assert!(super::apfs::refused(
            "Error unlocking APFS Volume: Passphrase incorrect or user does not exist"
        ));
        assert!(!super::apfs::refused(
            "Error unlocking APFS Volume: This operation is restricted by Sandbox (-69464)"
        ));
    }

    const LSBLK: &str = r#"{"blockdevices": [
        {"path":"/dev/sda","label":null,"size":500107862016,"fstype":null,"mountpoint":null,"hotplug":true,"pkname":null},
        {"path":"/dev/sda1","label":null,"size":104857600,"fstype":"vfat","mountpoint":null,"hotplug":true,"pkname":"sda"},
        {"path":"/dev/sda3","label":"Windows","size":499000000000,"fstype":"ntfs","mountpoint":null,"hotplug":true,"pkname":"sda"},
        {"path":"/dev/sdb2","label":null,"size":3840000000000,"fstype":"crypto_LUKS","mountpoint":null,"hotplug":true,"pkname":"sdb"},
        {"path":"/dev/sdb","label":null,"size":4001000000000,"fstype":null,"mountpoint":null,"hotplug":true,"pkname":null},
        {"path":"/dev/sdc","label":null,"size":2000000000000,"fstype":"crypto_LUKS","mountpoint":null,"hotplug":false,"pkname":null},
        {"path":"/dev/mapper/luks-example","label":"Backup 4T","size":3840000000000,"fstype":"btrfs","mountpoint":null,"hotplug":false,"pkname":"sdb2"},
        {"path":"/dev/nvme0n1p2","label":null,"size":1024000000000,"fstype":"crypto_LUKS","mountpoint":null,"hotplug":false,"pkname":"nvme0n1"},
        {"path":"/dev/mapper/root","label":"root","size":1024000000000,"fstype":"btrfs","mountpoint":"/","hotplug":false,"pkname":"nvme0n1p2"},
        {"path":"/dev/nvme0n1p3","label":null,"size":8000000000,"fstype":"swap","mountpoint":null,"hotplug":false,"pkname":"nvme0n1"}
    ]}"#;

    #[test]
    fn unmounted_volumes_are_listed_without_members_and_system_partitions_marked() {
        let volumes = super::parse_lsblk(LSBLK);
        let devices: Vec<&str> = volumes.iter().map(|v| v.device.as_str()).collect();
        assert_eq!(
            devices,
            [
                "/dev/sda1",
                "/dev/sda3",
                "/dev/sdc",
                "/dev/mapper/luks-example"
            ],
            "no unlocked or in-use container, no swap, nothing mounted"
        );
        let (efi, volumes) = volumes.split_first().unwrap();
        assert!(efi.system, "a small FAT partition, where udev says nothing");
        assert!(volumes.iter().all(|v| !v.system));
        assert!(volumes[1].locked && !volumes[0].locked);
        assert!(
            volumes[2].hotplug,
            "inherited from the disk under the container"
        );
        assert_eq!(volumes[2].label, "Backup 4T");
        assert_eq!(volumes[2].container.as_deref(), Some("/dev/sdb2"));
        assert_eq!(volumes[0].container, None);
        assert!(super::parse_lsblk("not json").is_empty());
    }

    /// udisks's word on a volume is read where udev keeps it, and a
    /// container's cleartext volume is known by the container: one
    /// volume, locked or not.
    #[test]
    fn a_volume_is_known_by_its_filesystem_and_what_udisks_says_of_it() {
        let root = tempfile::tempdir().unwrap();
        let (sys, udev) = (root.path().join("sys"), root.path().join("udev"));
        let block = |name: &str, dev: &str, record: Option<&str>| {
            std::fs::create_dir_all(sys.join(name)).unwrap();
            std::fs::write(sys.join(name).join("dev"), format!("{dev}\n")).unwrap();
            if let Some(record) = record {
                std::fs::create_dir_all(&udev).unwrap();
                std::fs::write(udev.join(format!("b{dev}")), record).unwrap();
            }
        };
        block(
            "sda1",
            "8:1",
            Some(
                "S:disk/by-uuid/0A1B\nE:ID_FS_UUID=0A1B-2C3D\nE:UDISKS_IGNORE=1\nE:ID_FS_TYPE=vfat\n",
            ),
        );
        block("sda3", "8:3", Some("E:ID_FS_UUID=ABC\n"));
        block("sdb2", "8:18", Some("E:ID_FS_UUID=1111-luks\n"));
        block("dm-3", "254:3", Some("E:ID_FS_UUID=2222-ext4\n"));
        std::fs::create_dir_all(sys.join("dm-3/dm")).unwrap();
        std::fs::write(sys.join("dm-3/dm/uuid"), "CRYPT-LUKS2-1111-luks-x\n").unwrap();
        std::fs::create_dir_all(sys.join("dm-3/slaves/sdb2")).unwrap();
        block("sdc1", "8:33", None);
        let of = |name: &str| super::block_identity(&sys, &udev, std::ffi::OsStr::new(name));
        assert_eq!(
            of("sda1"),
            super::Identity {
                uuid: Some("0A1B-2C3D".into()),
                ignored: Some(true)
            }
        );
        assert_eq!(
            of("sda3").ignored,
            Some(false),
            "a record that says nothing"
        );
        assert_eq!(
            of("dm-3"),
            super::Identity {
                uuid: Some("1111-luks".into()),
                ignored: Some(false)
            },
            "the container's"
        );
        assert_eq!(of("sdc1"), super::Identity::default(), "no record");
        assert_eq!(of("nothing"), super::Identity::default());
        assert_eq!(super::identity("not a device"), super::Identity::default());
    }

    #[test]
    fn a_cleartext_device_names_its_container_and_udisks_messages_are_read() {
        assert_eq!(
            super::container_of(LSBLK, "/dev/mapper/luks-example").as_deref(),
            Some("/dev/sdb2")
        );
        assert_eq!(super::container_of(LSBLK, "/dev/sda3"), None);
        assert_eq!(
            super::mount_point_of("Mounted /dev/sdb1 at /run/media/example/USB stick\n"),
            Some(std::path::PathBuf::from("/run/media/example/USB stick"))
        );
        assert_eq!(
            super::cleartext_of("Unlocked /dev/sdb2 as /dev/dm-3.\n").as_deref(),
            Some("/dev/dm-3")
        );
    }

    // GTK's bookmarks (step F, ADR-035).
    #[cfg(target_os = "linux")]
    #[test]
    fn bookmarks_are_added_once_as_escaped_uris() {
        use super::BookmarkEdit::{Add, Remove};
        let path = std::path::Path::new("/example/Work files/it's 100% done");
        let uri = crate::uri::file_uri(path);
        assert_eq!(uri, "file:///example/Work%20files/it's%20100%25%20done");
        let add = Add {
            path: path.into(),
            title: None,
            before: None,
        };
        let (first, inverse) = super::edit_text("file:///example/other Other\n", &add).unwrap();
        assert_eq!(
            first,
            format!("file:///example/other Other\n{uri}\n"),
            "existing bookmarks and their labels are kept"
        );
        assert_eq!(inverse, Remove { path: path.into() });
        assert_eq!(super::edit_text(&first, &add), None);
        assert_eq!(
            super::edit_text(
                "file:///example/Work%20files/it%27s%20100%25%20done\n",
                &add
            ),
            None,
            "another application's escaping names the same folder"
        );
        assert_eq!(super::edit_text("", &add).unwrap().0, format!("{uri}\n"));
    }

    /// Every edit is undone by the edit it returns: the bookmarks come back
    /// in their order with their labels, and every line that is not a
    /// folder (remote bookmarks) survives the round trip, though it may end
    /// up on the other side of a neighbour. A removed bookmark comes back
    /// with our escaping of its URI, which names the same folder.
    // GTK's bookmarks (step F, ADR-035).
    #[cfg(target_os = "linux")]
    #[test]
    fn bookmark_edits_are_undone_by_their_inverse() {
        use super::BookmarkEdit::{Add, Move, Remove, Rename};
        use super::Marked::Folder;
        let text = "file:///example/a%20b A label\nsftp://host/x Remote\nfile:///example/it%27s\n\nfile:///example/c\n";
        let a = PathBuf::from("/example/a b");
        let its = PathBuf::from("/example/it's");
        let c = PathBuf::from("/example/c");
        let d = PathBuf::from("/example/Ünïcode d");
        let edits = [
            Remove { path: a.clone() },
            Remove { path: its.clone() },
            Rename {
                mark: Folder(its.clone()),
                title: Some("Reports".into()),
            },
            Rename {
                mark: Folder(a.clone()),
                title: None,
            },
            Move {
                mark: Folder(c.clone()),
                before: Some(Folder(a.clone())),
            },
            Move {
                mark: Folder(a.clone()),
                before: None,
            },
            Move {
                mark: Folder(a.clone()),
                before: Some(Folder(c.clone())),
            },
            Add {
                path: d.clone(),
                title: Some("Dee".into()),
                before: Some(Folder(its.clone())),
            },
        ];
        for edit in edits {
            let (changed, inverse) = super::edit_text(text, &edit)
                .unwrap_or_else(|| panic!("{edit:?} changes something"));
            assert_ne!(changed, text, "{edit:?}");
            let (back, redo) = super::edit_text(&changed, &inverse)
                .unwrap_or_else(|| panic!("{inverse:?} reverts {edit:?}"));
            let sorted = |text: &str| {
                let mut lines: Vec<&str> = text
                    .lines()
                    .filter(|l| {
                        crate::uri::path_of(l.split(' ').next().unwrap_or_default()).is_none()
                    })
                    .collect();
                lines.sort_unstable();
                lines.join("\n")
            };
            assert_eq!(parse_bookmarks(&back), parse_bookmarks(text), "{edit:?}");
            assert_eq!(sorted(&back), sorted(text), "{edit:?} then {inverse:?}");
            let (again, _) = super::edit_text(&back, &redo).expect("redo");
            assert_eq!(parse_bookmarks(&again), parse_bookmarks(&changed), "redo");
        }
    }

    // GTK's bookmarks (step F, ADR-035).
    #[cfg(target_os = "linux")]
    #[test]
    fn bookmark_edits_that_change_nothing_say_so() {
        use super::BookmarkEdit::{Move, Remove, Rename};
        use super::Marked::Folder;
        let text = "file:///example/a\nfile:///example/b Bee\n";
        let a = PathBuf::from("/example/a");
        let b = PathBuf::from("/example/b");
        for edit in [
            Remove {
                path: "/example/z".into(),
            },
            // Already where it is asked to go.
            Move {
                mark: Folder(a.clone()),
                before: Some(Folder(b.clone())),
            },
            Move {
                mark: Folder(b.clone()),
                before: None,
            },
            Move {
                mark: Folder(a.clone()),
                before: Some(Folder(a.clone())),
            },
            Rename {
                mark: Folder(b.clone()),
                title: Some("Bee".into()),
            },
            // The folder's own name is no label at all.
            Rename {
                mark: Folder(a.clone()),
                title: Some(" a ".into()),
            },
        ] {
            assert_eq!(super::edit_text(text, &edit), None, "{edit:?}");
        }
        let (renamed, _) = super::edit_text(
            text,
            &Rename {
                mark: Folder(b),
                title: Some("two\nlines ".into()),
            },
        )
        .unwrap();
        assert_eq!(renamed, "file:///example/a\nfile:///example/b two lines\n");
        let (moved, _) = super::edit_text(
            text,
            &Move {
                mark: Folder(a),
                before: None,
            },
        )
        .unwrap();
        assert_eq!(moved, "file:///example/b Bee\nfile:///example/a\n");
    }

    #[test]
    fn mounts_report_their_device() {
        let line = "36 35 98:0 / /run/media/example/USB\\040stick rw - vfat /dev/sdb1 rw";
        let mounts = super::parse_mountinfo(line);
        assert_eq!(mounts[0].source, "/dev/sdb1");
        assert!(mounts[0].udisks);
    }

    use super::*;

    /// A remote folder is bookmarked by its address, last, and taken out
    /// again by it, every other line as it was (ADR-077).
    #[test]
    fn an_address_is_bookmarked_and_taken_out_by_itself() {
        let text = "file:///home/example/Documents\nsftp://old@example.com/ Old\n";
        let address = "sftp://me@example.com/home/me/Two%20Words".to_owned();
        let (added, inverse) = edit_text(
            text,
            &BookmarkEdit::AddAddress {
                address: address.clone(),
                title: Some("Work".into()),
                before: None,
            },
        )
        .unwrap();
        assert_eq!(added, format!("{text}{address} Work\n"));
        assert_eq!(
            inverse,
            BookmarkEdit::RemoveAddress {
                address: address.clone()
            }
        );
        assert!(
            edit_text(
                &added,
                &BookmarkEdit::AddAddress {
                    address: address.clone(),
                    title: None,
                    before: None,
                }
            )
            .is_none(),
            "already bookmarked"
        );
        let (removed, back) = edit_text(&added, &inverse).unwrap();
        assert_eq!(removed, text);
        assert_eq!(
            back,
            BookmarkEdit::AddAddress {
                address,
                title: Some("Work".into()),
                before: None,
            }
        );
    }

    /// Folders and addresses are one list in the file's order: an address
    /// is renamed and moved among folders as a folder is, before one or
    /// after, and every edit's inverse gives the file back as it was, an
    /// address removed coming back where it stood.
    // GTK's bookmarks (step F, ADR-035).
    #[cfg(target_os = "linux")]
    #[test]
    fn addresses_and_folders_share_one_order() {
        let text = "file:///example/a\nsftp://me@example.com/docs Docs\nfile:///example/c See\nsmb://nas/photos%20x\n";
        let a = Marked::Folder("/example/a".into());
        let docs = Marked::Address("sftp://me@example.com/docs".into());
        let c = Marked::Folder("/example/c".into());
        let photos = Marked::Address("smb://nas/photos%20x".into());
        let order = |text: &str| -> Vec<String> {
            parse_every_bookmark(text)
                .into_iter()
                .map(|b| match b {
                    Bookmarked::Folder(b) => b.title,
                    Bookmarked::Address(b) => b.title,
                })
                .collect()
        };
        assert_eq!(order(text), ["a", "Docs", "See", "photos x"]);
        let edits = [
            BookmarkEdit::Move {
                mark: docs.clone(),
                before: Some(a.clone()),
            },
            BookmarkEdit::Move {
                mark: a.clone(),
                before: Some(photos.clone()),
            },
            BookmarkEdit::Move {
                mark: photos.clone(),
                before: Some(docs.clone()),
            },
            BookmarkEdit::Move {
                mark: docs.clone(),
                before: None,
            },
            BookmarkEdit::Rename {
                mark: docs.clone(),
                title: Some("Work\nfiles ".into()),
            },
            BookmarkEdit::Rename {
                mark: docs.clone(),
                title: None,
            },
            BookmarkEdit::Rename {
                mark: photos.clone(),
                title: Some("Photos".into()),
            },
            BookmarkEdit::RemoveAddress {
                address: "sftp://me@example.com/docs".into(),
            },
            BookmarkEdit::Remove {
                path: "/example/c".into(),
            },
            BookmarkEdit::AddAddress {
                address: "dav://nas/share".into(),
                title: None,
                before: Some(c.clone()),
            },
        ];
        for edit in edits {
            let (changed, inverse) =
                edit_text(text, &edit).unwrap_or_else(|| panic!("{edit:?} changes something"));
            let (back, _) = edit_text(&changed, &inverse)
                .unwrap_or_else(|| panic!("{inverse:?} reverts {edit:?}"));
            assert_eq!(back, text, "{edit:?} then {inverse:?}");
        }
        let moved = |edit| edit_text(text, &edit).unwrap().0;
        assert_eq!(
            order(&moved(BookmarkEdit::Move {
                mark: docs.clone(),
                before: Some(a.clone()),
            })),
            ["Docs", "a", "See", "photos x"]
        );
        assert_eq!(
            order(&moved(BookmarkEdit::Move {
                mark: a,
                before: Some(photos),
            })),
            ["Docs", "See", "a", "photos x"]
        );
        let (renamed, _) = edit_text(
            text,
            &BookmarkEdit::Rename {
                mark: docs.clone(),
                title: Some("Work\nfiles ".into()),
            },
        )
        .unwrap();
        assert!(renamed.contains("sftp://me@example.com/docs Work files\n"));
        // Where it already is, as it is already called: nothing to do.
        for edit in [
            BookmarkEdit::Move {
                mark: docs.clone(),
                before: Some(c),
            },
            BookmarkEdit::Rename {
                mark: docs,
                title: Some(" Docs ".into()),
            },
        ] {
            assert_eq!(edit_text(text, &edit), None, "{edit:?}");
        }
    }

    /// The bookmarks another file manager made of remote folders are
    /// read as their addresses (ADR-077), named by their label or their
    /// last part.
    #[test]
    fn remote_bookmarks_are_their_addresses() {
        let text = "file:///home/example/Documents\n\
            sftp://me@example.com/home/me Work\n\
            smb://nas/photos/2024%20Summer\n\
            \n";
        assert_eq!(
            parse_remote_bookmarks(text),
            [
                RemoteBookmark {
                    title: "Work".into(),
                    address: "sftp://me@example.com/home/me".into(),
                },
                RemoteBookmark {
                    title: "2024 Summer".into(),
                    address: "smb://nas/photos/2024%20Summer".into(),
                },
            ]
        );
    }

    #[test]
    fn user_dirs_parse() {
        let text = "# comment\nXDG_DESKTOP_DIR=\"$HOME/Bureau\"\nXDG_DOWNLOAD_DIR=\"$HOME/dl\"\nXDG_PROJECTS_DIR=\"$HOME/Projets\"\nXDG_MUSIC_DIR=\"$HOME\"\nGARBAGE\n";
        let v = parse_user_dirs(text, Path::new("/home/u"));
        assert_eq!(v.len(), 4);
        assert_eq!(
            v[0],
            UserDir {
                name: "Bureau".into(),
                path: "/home/u/Bureau".into(),
                kind: "desktop".into()
            }
        );
        assert_eq!(v[1].kind, "download");
        assert_eq!(v[2].kind, "projects");
        assert_eq!(v[3].path, Path::new("/home/u"));
    }

    // GTK's bookmarks (step F, ADR-035).
    #[cfg(target_os = "linux")]
    #[test]
    fn bookmarks_parse() {
        let v = parse_bookmarks(
            "file:///home/u/My%20Docs Docs\nfile:///srv/data\nsftp://host/x Remote\n\n",
        );
        assert_eq!(v.len(), 2);
        assert_eq!(
            v[0],
            Bookmark {
                title: "Docs".into(),
                path: "/home/u/My Docs".into()
            }
        );
        assert_eq!(
            v[1],
            Bookmark {
                title: "data".into(),
                path: "/srv/data".into()
            }
        );
    }

    #[test]
    fn mountinfo_parse() {
        let text = "\
22 1 0:20 / /proc rw,nosuid - proc proc rw
23 1 0:21 /root / rw,noatime shared:1 - btrfs /dev/nvme0n1p2 rw,compress=zstd:3
24 23 0:21 /home /home rw,noatime - btrfs /dev/nvme0n1p2 rw
25 23 0:25 / /nix/store ro,nosuid - btrfs /dev/nvme0n1p2 ro
26 23 0:30 / /run/media/u/my\\040disk rw,relatime - vfat /dev/sdb1 rw
27 23 0:31 / /tmp rw - tmpfs tmpfs rw
28 23 0:32 / /mnt/backup rw - fuse.sshfs user@host:/ rw
";
        let v = parse_mountinfo(text);
        let paths: Vec<String> = v.iter().map(|m| m.path.display().to_string()).collect();
        assert_eq!(paths, ["/", "/home", "/run/media/u/my disk", "/mnt/backup"]);
        assert_eq!(v[0].label.english(), "Root");
        assert!(!v[0].udisks);
        assert_eq!(v[2].label.english(), "my disk");
        assert!(v[2].udisks);
        assert_eq!(v[3].fstype, "fuse.sshfs");
        assert_eq!(v[0].root, Path::new("/root"), "a subvolume");
        assert_eq!(v[2].root, Path::new("/"));
    }

    #[test]
    #[cfg(target_os = "linux")]
    fn live_mounts_include_root() {
        assert!(mounts().iter().any(|m| m.path == Path::new("/")));
    }

    // Linux's mounts.
    #[cfg(target_os = "linux")]
    #[test]
    fn mount_points_keep_their_own_bytes_and_periods() {
        let mounts = parse_mountinfo(
            "26 23 0:30 / /run/media/u/Cl\\303\\251\\040A rw - vfat /dev/sdb1 rw\n",
        );
        assert_eq!(mounts[0].path, Path::new("/run/media/u/Clé A"));
        // As the kernel writes it: a byte that is not UTF-8 raw, which
        // must not empty the list.
        let raw: &[u8] = b"22 1 0:30 / / rw - btrfs /dev/root rw\n\
            26 23 0:30 / /mnt/caf\xe9 rw - vfat /dev/sdb1 rw\n";
        let mounts = parse_mountinfo(raw);
        assert_eq!(mounts.len(), 2);
        assert_eq!(
            mounts[1].path.as_os_str().as_encoded_bytes(),
            b"/mnt/caf\xe9"
        );
        let dir = tempfile::tempdir().unwrap();
        let label = dir.path().join("Backup.");
        std::fs::create_dir(&label).unwrap();
        let old = |path: &Path| format!("Mounted /dev/sdb1 at {}.\n", path.display());
        let new = |path: &Path| format!("Mounted /dev/sdb1 at {}\n", path.display());
        assert_eq!(mount_point_of(&old(&label)), Some(label.clone()));
        assert_eq!(mount_point_of(&new(&label)), Some(label));
        let other = tempfile::tempdir().unwrap();
        let plain = other.path().join("Backup");
        std::fs::create_dir(&plain).unwrap();
        assert_eq!(mount_point_of(&old(&plain)), Some(plain.clone()));
        assert_eq!(mount_point_of(&new(&plain)), Some(plain));
    }

    /// Windows' drives are the devices, each named with its letter.
    #[cfg(windows)]
    #[test]
    fn windows_lists_its_drives_and_the_users_folders() {
        let mounts = super::mounts();
        let c = mounts
            .iter()
            .find(|m| m.path == std::path::Path::new("C:\\"))
            .unwrap_or_else(|| panic!("{mounts:?}"));
        assert!(c.label.english().ends_with("(C:)"), "{}", c.label);
        assert!(super::space(&c.path).is_some());
        let dirs = super::user_dirs();
        assert!(
            dirs.iter()
                .any(|d| d.kind == "documents" || d.kind == "desktop"),
            "{dirs:?}"
        );
    }
}
