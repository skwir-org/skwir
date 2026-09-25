//! Places the user already knows: home, XDG user dirs, GTK bookmarks, mounts.

use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;

use bibli_core::CancelToken;

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
        Command::new("lsblk").args(LSBLK_ARGS),
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

pub fn unmounted_volumes() -> Vec<Volume> {
    lsblk().map(|json| parse_lsblk(&json)).unwrap_or_default()
}

/// Run `udisksctl <verb> --block-device <device>`, with `input` on its
/// standard input, and return what it printed.
fn udisks(
    verb: &str,
    device: &str,
    extra: &[&str],
    input: Option<&[u8]>,
) -> anyhow::Result<String> {
    let mut command = Command::new("udisksctl");
    command
        .arg(verb)
        .arg("--block-device")
        .arg(device)
        .arg("--no-user-interaction")
        .args(extra);
    let finished =
        crate::process::run_to_exit(&mut command, input, &CancelToken::new(), UDISKS_TIMEOUT)
            .map_err(|error| anyhow::anyhow!("udisksctl could not be run: {error:#}"))?;
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

/// Mount through udisks and return the mount point. Blocks: worker only.
pub fn mount(device: &str) -> anyhow::Result<PathBuf> {
    let message = udisks("mount", device, &[], None)?;
    mount_point_of(&message)
        .ok_or_else(|| anyhow::anyhow!("udisksctl did not say where it mounted the volume"))
}

/// Unlock an encrypted volume and return its cleartext device. The
/// passphrase travels on the child's standard input, never on a command
/// line, and is not kept. Blocks: worker only.
pub fn unlock(device: &str, passphrase: &str) -> anyhow::Result<String> {
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
    udisks("unmount", source, &[], None)?;
    if let Some(container) = container {
        udisks("lock", &container, &[], None)?;
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
    let Some(file) = bookmarks_file() else {
        return Vec::new();
    };
    let text = std::fs::read_to_string(file).unwrap_or_default();
    parse_bookmarks(&text)
}

/// Parse `file:///path%20x Label` lines. Non-`file://` entries are skipped.
pub fn parse_bookmarks(text: &str) -> Vec<Bookmark> {
    lines_of(text)
        .into_iter()
        .filter_map(|line| line.mark)
        .map(|(path, _, label)| {
            let title = label
                .or_else(|| path.file_name().map(|n| n.to_string_lossy().into_owned()))
                .unwrap_or_else(|| path.display().to_string());
            Bookmark { title, path }
        })
        .collect()
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
        before: Option<PathBuf>,
    },
    Remove {
        path: PathBuf,
    },
    /// Label `path`'s bookmark `title`; `None` is the folder's own name.
    Rename {
        path: PathBuf,
        title: Option<String>,
    },
    /// Put `path`'s bookmark before `before`'s, last when `None`.
    Move {
        path: PathBuf,
        before: Option<PathBuf>,
    },
}

impl BookmarkEdit {
    /// The folder the edit is about.
    pub fn path(&self) -> &Path {
        match self {
            Self::Add { path, .. }
            | Self::Remove { path }
            | Self::Rename { path, .. }
            | Self::Move { path, .. } => path,
        }
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

fn position(lines: &[Line], path: &Path) -> Option<usize> {
    lines
        .iter()
        .position(|l| l.mark.as_ref().is_some_and(|(p, ..)| p == path))
}

/// The folder bookmarked after line `index`, other than `path` itself.
fn next_after(lines: &[Line], index: usize, path: &Path) -> Option<PathBuf> {
    lines[index + 1..]
        .iter()
        .filter_map(|l| l.mark.as_ref().map(|(p, ..)| p))
        .find(|p| *p != path)
        .cloned()
}

/// Where a line goes to stand before `before`'s bookmark: at its line, or
/// at the end.
fn insertion(lines: &[Line], before: Option<&Path>) -> usize {
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
            if position(&lines, path).is_some() {
                return None;
            }
            let uri = crate::uri::file_uri(path);
            let label = label_for(path, title.as_deref());
            let at = insertion(&lines, before.as_deref());
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
            let first = position(&lines, path)?;
            let before = next_after(&lines, first, path);
            let title = lines[first].mark.as_ref().and_then(|(.., l)| l.clone());
            lines.retain(|l| l.mark.as_ref().is_none_or(|(p, ..)| p != path));
            BookmarkEdit::Add {
                path: path.clone(),
                title,
                before,
            }
        }
        BookmarkEdit::Rename { path, title } => {
            let at = position(&lines, path)?;
            let label = label_for(path, title.as_deref());
            let line = &mut lines[at];
            let (_, uri, old) = line.mark.as_mut()?;
            if *old == label {
                return None;
            }
            line.raw = mark_line(uri, label.as_deref());
            let old = std::mem::replace(old, label);
            BookmarkEdit::Rename {
                path: path.clone(),
                title: old,
            }
        }
        BookmarkEdit::Move { path, before } => {
            let from = position(&lines, path)?;
            let was = next_after(&lines, from, path);
            if before.as_deref() == Some(path.as_path()) || was == *before {
                return None;
            }
            let line = lines.remove(from);
            let at = insertion(&lines, before.as_deref());
            lines.insert(at, line);
            BookmarkEdit::Move {
                path: path.clone(),
                before: was,
            }
        }
    };
    Some((text_of(&lines), inverse))
}

/// The GTK bookmarks file every file manager and file dialog on the desktop
/// shares, as it is named (it may be a link).
pub fn bookmarks_file() -> Option<PathBuf> {
    Some(dirs::config_dir()?.join("gtk-3.0").join("bookmarks"))
}

/// The folders to watch to see the bookmarks change: the file's own, and
/// the one it points into when it is a link (a dotfile manager's), since
/// writers replace the file rather than write into it.
pub fn bookmarks_folders() -> Vec<PathBuf> {
    let Some(file) = bookmarks_file() else {
        return Vec::new();
    };
    let mut folders: Vec<PathBuf> = file.parent().map(Path::to_path_buf).into_iter().collect();
    if let Some(target) = std::fs::canonicalize(&file)
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
/// is followed and a read-only file refused ([`crate::file::rewrite`]).
pub fn edit_bookmarks(edit: &BookmarkEdit) -> anyhow::Result<Option<BookmarkEdit>> {
    let link = bookmarks_file().ok_or_else(|| anyhow::anyhow!("no configuration directory"))?;
    crate::file::rewrite(&link, "the bookmarks file", |text| edit_text(text, edit))
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
            source: rf
                .get(1)
                .map(|s| unescape_mountinfo(s).to_string_lossy().into_owned())
                .unwrap_or_default(),
        });
    }
    out
}

/// mountinfo escapes space, tab, newline and backslash as octal `\040` etc.
/// Every other byte is the path's own, UTF-8 or not.
fn unescape_mountinfo(s: &str) -> OsString {
    let mut out = Vec::with_capacity(s.len());
    let b = s.as_bytes();
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'\\'
            && i + 3 < b.len()
            && let Ok(v) =
                u8::from_str_radix(std::str::from_utf8(&b[i + 1..i + 4]).unwrap_or("zzz"), 8)
        {
            out.push(v);
            i += 4;
            continue;
        }
        out.push(b[i]);
        i += 1;
    }
    os_string(out)
}

#[cfg(unix)]
fn os_string(bytes: Vec<u8>) -> OsString {
    std::os::unix::ffi::OsStringExt::from_vec(bytes)
}

#[cfg(not(unix))]
fn os_string(bytes: Vec<u8>) -> OsString {
    String::from_utf8_lossy(&bytes).into_owned().into()
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
    #[test]
    fn bookmark_edits_are_undone_by_their_inverse() {
        use super::BookmarkEdit::{Add, Move, Remove, Rename};
        let text = "file:///example/a%20b A label\nsftp://host/x Remote\nfile:///example/it%27s\n\nfile:///example/c\n";
        let a = PathBuf::from("/example/a b");
        let its = PathBuf::from("/example/it's");
        let c = PathBuf::from("/example/c");
        let d = PathBuf::from("/example/Ünïcode d");
        let edits = [
            Remove { path: a.clone() },
            Remove { path: its.clone() },
            Rename {
                path: its.clone(),
                title: Some("Reports".into()),
            },
            Rename {
                path: a.clone(),
                title: None,
            },
            Move {
                path: c.clone(),
                before: Some(a.clone()),
            },
            Move {
                path: a.clone(),
                before: None,
            },
            Move {
                path: a.clone(),
                before: Some(c.clone()),
            },
            Add {
                path: d.clone(),
                title: Some("Dee".into()),
                before: Some(its.clone()),
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

    #[test]
    fn bookmark_edits_that_change_nothing_say_so() {
        use super::BookmarkEdit::{Move, Remove, Rename};
        let text = "file:///example/a\nfile:///example/b Bee\n";
        let a = PathBuf::from("/example/a");
        let b = PathBuf::from("/example/b");
        for edit in [
            Remove {
                path: "/example/z".into(),
            },
            // Already where it is asked to go.
            Move {
                path: a.clone(),
                before: Some(b.clone()),
            },
            Move {
                path: b.clone(),
                before: None,
            },
            Move {
                path: a.clone(),
                before: Some(a.clone()),
            },
            Rename {
                path: b.clone(),
                title: Some("Bee".into()),
            },
            // The folder's own name is no label at all.
            Rename {
                path: a.clone(),
                title: Some(" a ".into()),
            },
        ] {
            assert_eq!(super::edit_text(text, &edit), None, "{edit:?}");
        }
        let (renamed, _) = super::edit_text(
            text,
            &Rename {
                path: b,
                title: Some("two\nlines ".into()),
            },
        )
        .unwrap();
        assert_eq!(renamed, "file:///example/a\nfile:///example/b two lines\n");
        let (moved, _) = super::edit_text(
            text,
            &Move {
                path: a,
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
    fn mount_points_keep_their_own_bytes_and_periods() {
        let mounts = parse_mountinfo(
            "26 23 0:30 / /run/media/u/Cl\\303\\251\\040A rw - vfat /dev/sdb1 rw\n",
        );
        assert_eq!(mounts[0].path, Path::new("/run/media/u/Clé A"));
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
}
