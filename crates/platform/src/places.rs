//! Places the user already knows: home, XDG user dirs, GTK bookmarks, mounts.

use std::path::{Path, PathBuf};

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
    pub label: String,
    pub path: PathBuf,
    pub fstype: String,
    pub removable: bool,
    /// The mounted device (`/dev/sdb1`), as the kernel reports it.
    pub source: String,
}

/// Free and total bytes of the filesystem holding `path`. Asks the
/// filesystem, which can block on a sleeping or unreachable volume: call it
/// from a worker.
pub fn space(path: &Path) -> Option<(u64, u64)> {
    let stat = rustix::fs::statvfs(path).ok()?;
    let total = stat.f_blocks.checked_mul(stat.f_frsize)?;
    (total > 0).then(|| (stat.f_bavail.saturating_mul(stat.f_frsize), total))
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
/// desktops hide too.
const SYSTEM_PARTITION_MAX: u64 = 2 << 30;

fn device_name(path: &str) -> &str {
    path.rsplit('/').next().unwrap_or(path)
}

/// Volumes from `lsblk --json --list` that could be mounted and are not:
/// filesystems without a mount point, and locked encrypted containers.
/// Members of something else (swap, RAID, LVM, an unlocked container) and
/// small system partitions are left out.
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
            if matches!(fstype, "vfat" | "ntfs") && size < SYSTEM_PARTITION_MAX {
                return None;
            }
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
            Some(Volume {
                device: device.path.clone(),
                label: device.label.clone().unwrap_or_default(),
                size,
                locked,
                hotplug,
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

/// Run `lsblk` and return its JSON. Spawns a process: call from a worker.
pub fn lsblk() -> Option<String> {
    let output = std::process::Command::new("lsblk")
        .args(LSBLK_ARGS)
        .output()
        .ok()?;
    output
        .status
        .success()
        .then(|| String::from_utf8_lossy(&output.stdout).into_owned())
}

pub fn unmounted_volumes() -> Vec<Volume> {
    lsblk().map(|json| parse_lsblk(&json)).unwrap_or_default()
}

fn udisks(verb: &str, device: &str) -> std::process::Command {
    let mut command = std::process::Command::new("udisksctl");
    command
        .arg(verb)
        .arg("--block-device")
        .arg(device)
        .arg("--no-user-interaction");
    command
}

fn finished(output: std::io::Result<std::process::Output>) -> anyhow::Result<String> {
    let output = output.map_err(|error| anyhow::anyhow!("udisksctl could not be run: {error}"))?;
    anyhow::ensure!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr).trim().to_owned()
    );
    Ok(String::from_utf8_lossy(&output.stdout).into_owned())
}

/// `Mounted /dev/sdb1 at /run/media/example/USB stick` gives the mount point.
pub fn mount_point_of(message: &str) -> Option<PathBuf> {
    let (_, path) = message.trim().split_once(" at ")?;
    Some(PathBuf::from(path.trim_end_matches('.')))
}

/// `Unlocked /dev/sdb2 as /dev/dm-3.` gives the cleartext device.
pub fn cleartext_of(message: &str) -> Option<String> {
    let (_, device) = message.trim().split_once(" as ")?;
    Some(device.trim_end_matches('.').to_owned())
}

/// Mount through udisks and return the mount point. Blocks: worker only.
pub fn mount(device: &str) -> anyhow::Result<PathBuf> {
    let message = finished(udisks("mount", device).output())?;
    mount_point_of(&message)
        .ok_or_else(|| anyhow::anyhow!("udisksctl did not say where it mounted the volume"))
}

/// Unlock an encrypted volume and return its cleartext device. The
/// passphrase travels on the child's standard input, never on a command
/// line, and is not kept. Blocks: worker only.
pub fn unlock(device: &str, passphrase: &str) -> anyhow::Result<String> {
    use std::io::Write as _;
    let mut child = udisks("unlock", device)
        .arg("--key-file")
        .arg("/dev/stdin")
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .map_err(|error| anyhow::anyhow!("udisksctl could not be run: {error}"))?;
    if let Some(mut stdin) = child.stdin.take() {
        // No trailing newline: a key file is taken byte for byte.
        stdin.write_all(passphrase.as_bytes())?;
    }
    let message = finished(child.wait_with_output()).map_err(|error| {
        let text = error.to_string();
        if text.contains("Failed to activate") || text.contains("No key available") {
            anyhow::anyhow!("The passphrase was not accepted")
        } else {
            error
        }
    })?;
    cleartext_of(&message)
        .ok_or_else(|| anyhow::anyhow!("udisksctl did not name the unlocked device"))
}

/// Unmount, then lock the container under it if there is one, so the volume
/// is safe to unplug and asks for its passphrase next time. Blocks: worker only.
pub fn unmount(source: &str) -> anyhow::Result<()> {
    let container = lsblk().and_then(|json| container_of(&json, source));
    finished(udisks("unmount", source).output())?;
    if let Some(container) = container {
        finished(udisks("lock", &container).output())?;
    }
    Ok(())
}

pub fn home() -> Option<PathBuf> {
    dirs::home_dir()
}

/// XDG user dirs from `~/.config/user-dirs.dirs`, existing ones only, `$HOME` itself excluded.
pub fn user_dirs() -> Vec<UserDir> {
    let Some(home) = home() else {
        return Vec::new();
    };
    let Some(cfg) = dirs::config_dir() else {
        return Vec::new();
    };
    let text = std::fs::read_to_string(cfg.join("user-dirs.dirs")).unwrap_or_default();
    parse_user_dirs(&text, &home)
        .into_iter()
        .filter(|d| d.path != home && d.path.is_dir())
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
    let Some(cfg) = dirs::config_dir() else {
        return Vec::new();
    };
    let text = std::fs::read_to_string(cfg.join("gtk-3.0/bookmarks")).unwrap_or_default();
    parse_bookmarks(&text)
}

/// Parse `file:///path%20x Label` lines. Non-`file://` entries are skipped.
pub fn parse_bookmarks(text: &str) -> Vec<Bookmark> {
    text.lines()
        .map(str::trim)
        .filter(|l| !l.is_empty())
        .filter_map(|l| {
            let (uri, label) = match l.split_once(' ') {
                Some((u, l)) => (u, Some(l.trim())),
                None => (l, None),
            };
            let raw = uri.strip_prefix("file://")?;
            let path = PathBuf::from(percent_decode(raw));
            let title = label
                .filter(|s| !s.is_empty())
                .map(str::to_owned)
                .or_else(|| path.file_name().map(|n| n.to_string_lossy().into_owned()))
                .unwrap_or_else(|| path.display().to_string());
            Some(Bookmark { title, path })
        })
        .collect()
}

/// Decode `%XX` sequences into bytes (lossy UTF-8).
pub fn percent_decode(s: &str) -> String {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'%'
            && i + 2 < b.len()
            && let Ok(v) =
                u8::from_str_radix(std::str::from_utf8(&b[i + 1..i + 3]).unwrap_or("zz"), 16)
        {
            out.push(v);
            i += 3;
            continue;
        }
        out.push(b[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// `file://` URI of a path, escaping what the bookmarks format and URIs
/// reserve (space, `%`, control and non-ASCII bytes).
pub fn file_uri(path: &Path) -> String {
    use std::fmt::Write as _;
    let mut uri = String::from("file://");
    for byte in path.as_os_str().as_encoded_bytes() {
        if byte.is_ascii_alphanumeric() || b"/-._~!$&'()*+,;=:@".contains(byte) {
            uri.push(*byte as char);
        } else {
            let _ = write!(uri, "%{byte:02X}");
        }
    }
    uri
}

/// The bookmarks text with `path` added, or `None` if it is already there.
pub fn with_bookmark(text: &str, path: &Path) -> Option<String> {
    let uri = file_uri(path);
    if text
        .lines()
        .any(|line| line.split(' ').next() == Some(uri.as_str()))
    {
        return None;
    }
    let mut out = text.to_owned();
    if !out.is_empty() && !out.ends_with('\n') {
        out.push('\n');
    }
    out.push_str(&uri);
    out.push('\n');
    Some(out)
}

/// Add a folder to the GTK bookmarks every file manager and file dialog on
/// the desktop shares. Returns false if it was already bookmarked. Writes a
/// file: call from a worker.
pub fn add_bookmark(path: &Path) -> anyhow::Result<bool> {
    use std::io::Write as _;
    let dir = dirs::config_dir()
        .ok_or_else(|| anyhow::anyhow!("no configuration directory"))?
        .join("gtk-3.0");
    let file = dir.join("bookmarks");
    let text = match std::fs::read_to_string(&file) {
        Ok(text) => text,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => String::new(),
        Err(error) => return Err(error.into()),
    };
    let Some(updated) = with_bookmark(&text, path) else {
        return Ok(false);
    };
    std::fs::create_dir_all(&dir)?;
    let mut staged = tempfile::NamedTempFile::new_in(&dir)?;
    staged.write_all(updated.as_bytes())?;
    staged.persist(&file)?;
    Ok(true)
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

/// Parse `/proc/self/mountinfo` content.
pub fn parse_mountinfo(text: &str) -> Vec<Mount> {
    let mut out: Vec<Mount> = Vec::new();
    for line in text.lines() {
        // 36 35 98:0 /mnt1 /mnt2 rw,noatime master:1 - ext3 /dev/root rw,errors=continue
        let Some((left, right)) = line.split_once(" - ") else {
            continue;
        };
        let lf: Vec<&str> = left.split(' ').collect();
        let rf: Vec<&str> = right.split(' ').collect();
        if lf.len() < 5 || rf.is_empty() {
            continue;
        }
        let mountpoint = PathBuf::from(unescape_mountinfo(lf[4]));
        let fstype = rf[0].to_owned();
        if !is_user_fs(&fstype) || !is_interesting_mountpoint(&mountpoint) {
            continue;
        }
        if out.iter().any(|m| m.path == mountpoint) {
            continue;
        }
        let label = if mountpoint == Path::new("/") {
            "Root".to_owned()
        } else {
            mountpoint
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_default()
        };
        let removable = mountpoint.starts_with("/run/media") || mountpoint.starts_with("/media");
        out.push(Mount {
            label,
            path: mountpoint,
            fstype,
            removable,
            source: rf.get(1).map(|s| unescape_mountinfo(s)).unwrap_or_default(),
        });
    }
    out
}

/// mountinfo escapes space, tab, newline and backslash as octal `\040` etc.
fn unescape_mountinfo(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let b = s.as_bytes();
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'\\'
            && i + 3 < b.len()
            && let Ok(v) =
                u8::from_str_radix(std::str::from_utf8(&b[i + 1..i + 4]).unwrap_or("zzz"), 8)
        {
            out.push(v as char);
            i += 4;
            continue;
        }
        out.push(b[i] as char);
        i += 1;
    }
    out
}

#[cfg(target_os = "linux")]
mod imp {
    use super::{Mount, parse_mountinfo};

    pub fn mounts() -> Vec<Mount> {
        std::fs::read_to_string("/proc/self/mountinfo")
            .map(|t| parse_mountinfo(&t))
            .unwrap_or_default()
    }
}

#[cfg(not(target_os = "linux"))]
mod imp {
    use super::Mount;

    pub fn mounts() -> Vec<Mount> {
        Vec::new()
    }
}

#[cfg(test)]
mod tests {
    const LSBLK: &str = r#"{"blockdevices": [
        {"path":"/dev/sda","label":null,"size":500107862016,"fstype":null,"mountpoint":null,"hotplug":true,"pkname":null},
        {"path":"/dev/sda1","label":null,"size":104857600,"fstype":"vfat","mountpoint":null,"hotplug":true,"pkname":"sda"},
        {"path":"/dev/sda3","label":"Windows","size":499000000000,"fstype":"ntfs","mountpoint":null,"hotplug":true,"pkname":"sda"},
        {"path":"/dev/sdb2","label":null,"size":3840000000000,"fstype":"crypto_LUKS","mountpoint":null,"hotplug":true,"pkname":"sdb"},
        {"path":"/dev/sdb","label":null,"size":4001000000000,"fstype":null,"mountpoint":null,"hotplug":true,"pkname":null},
        {"path":"/dev/sdc","label":null,"size":2000000000000,"fstype":"crypto_LUKS","mountpoint":null,"hotplug":false,"pkname":null},
        {"path":"/dev/mapper/luks-example","label":"Backup 4T","size":3840000000000,"fstype":"btrfs","mountpoint":null,"hotplug":false,"pkname":"sdb2"},
        {"path":"/dev/nvme0n1p2","label":null,"size":1024000000000,"fstype":"crypto_LUKS","mountpoint":null,"hotplug":false,"pkname":"nvme0n1"},
        {"path":"/dev/mapper/cryptroot","label":"root","size":1024000000000,"fstype":"btrfs","mountpoint":"/","hotplug":false,"pkname":"nvme0n1p2"},
        {"path":"/dev/nvme0n1p3","label":null,"size":8000000000,"fstype":"swap","mountpoint":null,"hotplug":false,"pkname":"nvme0n1"}
    ]}"#;

    #[test]
    fn unmounted_volumes_are_listed_without_members_and_system_partitions() {
        let volumes = super::parse_lsblk(LSBLK);
        let devices: Vec<&str> = volumes.iter().map(|v| v.device.as_str()).collect();
        assert_eq!(
            devices,
            ["/dev/sda3", "/dev/sdc", "/dev/mapper/luks-example"],
            "no EFI partition, no unlocked or in-use container, no swap, nothing mounted"
        );
        assert!(volumes[1].locked && !volumes[0].locked);
        assert!(
            volumes[2].hotplug,
            "inherited from the disk under the container"
        );
        assert_eq!(volumes[2].label, "Backup 4T");
        assert!(super::parse_lsblk("not json").is_empty());
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

    #[test]
    fn bookmarks_are_added_once_as_escaped_uris() {
        let path = std::path::Path::new("/example/Work files/it's 100% done");
        let uri = super::file_uri(path);
        assert_eq!(uri, "file:///example/Work%20files/it's%20100%25%20done");
        assert_eq!(
            super::percent_decode(&uri["file://".len()..]),
            path.to_string_lossy()
        );
        let first = super::with_bookmark("file:///example/other Other\n", path).unwrap();
        assert_eq!(
            first,
            format!("file:///example/other Other\n{uri}\n"),
            "existing bookmarks and their labels are kept"
        );
        assert_eq!(super::with_bookmark(&first, path), None);
        assert_eq!(super::with_bookmark("", path).unwrap(), format!("{uri}\n"));
    }

    #[test]
    fn mounts_report_their_device() {
        let line = "36 35 98:0 / /run/media/example/USB\\040stick rw - vfat /dev/sdb1 rw";
        let mounts = super::parse_mountinfo(line);
        assert_eq!(mounts[0].source, "/dev/sdb1");
        assert!(mounts[0].removable);
    }

    use super::*;

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
        assert_eq!(v[0].label, "Root");
        assert!(!v[0].removable);
        assert_eq!(v[2].label, "my disk");
        assert!(v[2].removable);
        assert_eq!(v[3].fstype, "fuse.sshfs");
    }

    #[test]
    #[cfg(target_os = "linux")]
    fn live_mounts_include_root() {
        assert!(mounts().iter().any(|m| m.path == Path::new("/")));
    }

    #[test]
    fn percent_decode_edges() {
        assert_eq!(percent_decode("a%20b%zz%4"), "a b%zz%4");
        assert_eq!(percent_decode("%C3%A9"), "é");
    }
}
