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
const MSDOS: u32 = 0x4d44;
const EXFAT: u32 = 0x2011_bab0;
/// The kernel's `ntfs3`.
const NTFS: u32 = 0x7366_746e;
/// Any FUSE filesystem: `ntfs-3g`, `exfat-fuse`, `sshfs`.
const FUSE: u32 = 0x6573_5546;
const SMB: u32 = 0x517b;
const SMB2: u32 = 0xfe53_4d42;
const CIFS: u32 = 0xff53_4d42;
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_home_folder_is_not_where_a_mac_writes_its_attributes_aside() {
        // The tests run on the developer's or CI's disk (ext4, btrfs,
        // tmpfs): none of those.
        let home = dirs::home_dir().unwrap();
        assert!(!apple_double(&home));
        assert!(!apple_double(Path::new("/no/such/folder")));
    }
}
