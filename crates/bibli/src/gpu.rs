//! Keep the Vulkan loader from loading drivers for hardware that is not
//! here. GPUI enumerates devices at startup, and the loader then maps every
//! installed ICD (about 40 MB of resident code on a typical Mesa install).
//! See ADR-002.

use std::collections::BTreeSet;
use std::path::Path;

/// PCI vendor ids as sysfs prints them.
const NVIDIA: &str = "0x10de";
const INTEL: &str = "0x8086";
const AMD: &str = "0x1002";
const VIRTIO: &str = "0x1af4";

/// `ICD` manifest globs that never apply on a desktop Linux machine: ARM `SoC`
/// GPUs, the Windows Direct3D layer, and the Android emulator stream.
const NEVER_HERE: &[&str] = &[
    "asahi_icd*",
    "dzn_icd*",
    "gfxstream*",
    "powervr*",
    "panfrost*",
    "freedreno*",
    "broadcom*",
];

/// Given the vendors present, the ICD manifests to disable. `None` if no
/// hardware GPU was found, in which case nothing is restricted so the
/// software rasterizer stays available.
pub fn drivers_to_disable(vendors: &BTreeSet<String>) -> Option<Vec<&'static str>> {
    let has = |v: &str| vendors.contains(v);
    if !(has(NVIDIA) || has(INTEL) || has(AMD) || has(VIRTIO)) {
        return None;
    }
    let mut out: Vec<&'static str> = NEVER_HERE.to_vec();
    // A hardware GPU is present: the software rasterizer is only weight.
    out.push("lvp_icd*");
    if !has(NVIDIA) {
        out.extend(["nvidia_icd*", "nouveau_icd*"]);
    }
    if !has(INTEL) {
        out.extend(["intel_icd*", "intel_hasvk*"]);
    }
    if !has(AMD) {
        out.push("radeon_icd*");
    }
    if !has(VIRTIO) {
        out.push("virtio_icd*");
    }
    Some(out)
}

/// Vendors of the DRM cards under `sys_class_drm`.
pub fn present_vendors(sys_class_drm: &Path) -> BTreeSet<String> {
    let mut out = BTreeSet::new();
    let Ok(rd) = std::fs::read_dir(sys_class_drm) else {
        return out;
    };
    for e in rd.flatten() {
        let name = e.file_name();
        let name = name.to_string_lossy();
        if !name.starts_with("card") || name.contains('-') {
            continue;
        }
        if let Ok(v) = std::fs::read_to_string(e.path().join("device/vendor")) {
            out.insert(v.trim().to_ascii_lowercase());
        }
    }
    out
}

/// Set `VK_LOADER_DRIVERS_DISABLE` unless the user already did. Call before
/// any Vulkan initialization. Returns what it set, for the caller to log once
/// logging exists: this runs before the subscriber is installed.
///
/// Setting an environment variable is unsafe in edition 2024 because other
/// threads may be reading the environment; this runs first thing in `main`,
/// before any thread exists, which is the one situation where it is sound.
#[allow(unsafe_code)]
pub fn restrict_vulkan_drivers() -> Option<String> {
    if std::env::var_os("VK_LOADER_DRIVERS_DISABLE").is_some()
        || std::env::var_os("VK_DRIVER_FILES").is_some()
        || std::env::var_os("VK_ICD_FILENAMES").is_some()
    {
        return None;
    }
    let vendors = present_vendors(Path::new("/sys/class/drm"));
    let value = drivers_to_disable(&vendors)?.join(",");
    // SAFETY: called from `main` before any thread is spawned.
    unsafe { std::env::set_var("VK_LOADER_DRIVERS_DISABLE", &value) };
    Some(value)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn set(v: &[&str]) -> BTreeSet<String> {
        v.iter().map(|s| (*s).to_owned()).collect()
    }

    #[test]
    fn keeps_present_vendors_and_drops_the_rest() {
        let d = drivers_to_disable(&set(&[NVIDIA, INTEL])).unwrap();
        assert!(
            d.contains(&"lvp_icd*") && d.contains(&"radeon_icd*") && d.contains(&"virtio_icd*")
        );
        assert!(
            !d.iter()
                .any(|g| g.starts_with("nvidia") || g.starts_with("intel"))
        );
    }

    #[test]
    fn no_hardware_means_no_restriction() {
        assert!(drivers_to_disable(&set(&[])).is_none());
        assert!(drivers_to_disable(&set(&["0xdead"])).is_none());
    }

    #[test]
    fn virtio_keeps_its_driver() {
        let d = drivers_to_disable(&set(&[VIRTIO])).unwrap();
        assert!(!d.contains(&"virtio_icd*"));
        assert!(d.contains(&"nvidia_icd*"));
    }

    #[test]
    fn reads_vendors_from_sysfs_layout() {
        let tmp = tempfile::tempdir().unwrap();
        for (card, vendor) in [("card0", "0x10DE\n"), ("card1", "0x8086\n")] {
            std::fs::create_dir_all(tmp.path().join(card).join("device")).unwrap();
            std::fs::write(tmp.path().join(card).join("device/vendor"), vendor).unwrap();
        }
        std::fs::create_dir_all(tmp.path().join("card0-DP-1")).unwrap();
        assert_eq!(present_vendors(tmp.path()), set(&["0x10de", "0x8086"]));
    }
}
