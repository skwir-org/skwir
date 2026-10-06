//! What kind of filesystem a folder is on, where that decides how a file's
//! notes are kept.
//!
//! A Mac writing to a filesystem that cannot hold its attributes keeps
//! them beside each file, in an `AppleDouble` file named `._` and the file's
//! name: on FAT and exFAT (USB sticks, SD cards), NTFS, and the network
//! and FUSE filesystems that do not take them. Where the filesystem does
//! (ext4, btrfs, XFS, APFS, HFS+), a `._` file is only what a copy left
//! behind, and nobody keeps it in step.

use std::path::Path;

/// Filesystem magic numbers (`statfs`'s `f_type`, from `linux/magic.h`
/// and the drivers).
#[cfg(target_os = "linux")]
const MSDOS: u32 = 0x4d44;
#[cfg(target_os = "linux")]
const EXFAT: u32 = 0x2011_bab0;
/// The kernel's `ntfs3`.
#[cfg(target_os = "linux")]
const NTFS: u32 = 0x7366_746e;
/// Any FUSE filesystem: `ntfs-3g`, `exfat-fuse`, `sshfs`.
#[cfg(target_os = "linux")]
const FUSE: u32 = 0x6573_5546;
#[cfg(target_os = "linux")]
const SMB: u32 = 0x517b;
#[cfg(target_os = "linux")]
const SMB2: u32 = 0xfe53_4d42;
#[cfg(target_os = "linux")]
const CIFS: u32 = 0xff53_4d42;
#[cfg(target_os = "linux")]
const NFS: u32 = 0x6969;

/// Whether a Mac keeps the attributes of files in `dir` in `._` files, as
/// it does on the filesystems listed above; false when it cannot be told.
/// One `statfs`: ask it once per folder, not per file.
pub fn apple_double(dir: &Path) -> bool {
    #[cfg(target_os = "linux")]
    {
        let Ok(fs) = rustix::fs::statfs(dir) else {
            return false;
        };
        #[allow(
            clippy::cast_possible_truncation,
            clippy::cast_sign_loss,
            reason = "a filesystem's magic number is 32 bits in a wider field"
        )]
        let magic = fs.f_type as u32;
        matches!(magic, MSDOS | EXFAT | NTFS | FUSE | SMB | SMB2 | CIFS | NFS)
    }
    #[cfg(not(target_os = "linux"))]
    {
        let _ = dir;
        false
    }
}

/// Filesystems that change a file's change time (`ctime`) whenever an
/// attribute is set on it, as Linux's local ones do.
#[cfg(target_os = "linux")]
const EXT: u32 = 0xef53;
#[cfg(target_os = "linux")]
const BTRFS: u32 = 0x9123_683e;
#[cfg(target_os = "linux")]
const XFS: u32 = 0x5846_5342;
#[cfg(target_os = "linux")]
const F2FS: u32 = 0xf2f5_2010;
#[cfg(target_os = "linux")]
const TMPFS: u32 = 0x0102_1994;
#[cfg(target_os = "linux")]
const ZFS: u32 = 0x2fc1_2fc1;
#[cfg(target_os = "linux")]
const BCACHEFS: u32 = 0xca45_1a4e;

/// Whether the change times of files in `dir` tell when their notes
/// changed: on the local filesystems above; not on a network or FUSE one,
/// where they may not move when an attribute is set. False when it cannot
/// be told. One `statfs`: ask it once per filesystem, not per file.
pub fn change_times_tell(dir: &Path) -> bool {
    #[cfg(target_os = "linux")]
    {
        let Ok(fs) = rustix::fs::statfs(dir) else {
            return false;
        };
        #[allow(
            clippy::cast_possible_truncation,
            clippy::cast_sign_loss,
            reason = "a filesystem's magic number is 32 bits in a wider field"
        )]
        let magic = fs.f_type as u32;
        matches!(magic, EXT | BTRFS | XFS | F2FS | TMPFS | ZFS | BCACHEFS)
    }
    #[cfg(not(target_os = "linux"))]
    {
        let _ = dir;
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(target_os = "linux")]
    #[test]
    fn a_home_folders_change_times_tell() {
        // The tests run on the developer's or CI's disk (ext4, btrfs).
        assert!(change_times_tell(&dirs::home_dir().unwrap()));
        assert!(!change_times_tell(Path::new("/no/such/folder")));
    }

    #[test]
    fn a_home_folder_is_not_where_a_mac_writes_its_attributes_aside() {
        // The tests run on the developer's or CI's disk (ext4, btrfs,
        // tmpfs): none of those.
        let home = dirs::home_dir().unwrap();
        assert!(!apple_double(&home));
        assert!(!apple_double(Path::new("/no/such/folder")));
    }
}
