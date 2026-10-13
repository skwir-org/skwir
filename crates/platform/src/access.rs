//! What the system adds to a file's mode: POSIX ACLs (`ls` shows them as
//! a `+` after the mode), file capabilities (what `setcap` gave a program)
//! and a security label (`SELinux`). They are the system's, not notes: read here,
//! for the one file Properties shows, on a worker; never per row.
//!
//! All three are extended attributes in the `system.` and `security.`
//! namespaces, read through what a link leads to, as its permissions are.

use std::path::Path;

const ACL_ACCESS: &str = "system.posix_acl_access";
const ACL_DEFAULT: &str = "system.posix_acl_default";
const CAPABILITIES: &str = "security.capability";
const SELINUX: &str = "security.selinux";

/// The facts of `path` beyond its mode, each with its label, in the
/// order Properties shows them; nothing for a file that has none.
pub fn facts(path: &Path) -> Vec<(&'static str, String)> {
    let Ok(names) = skwir_core::fs::xattr::list_deref(path) else {
        return Vec::new();
    };
    let names: Vec<String> = names
        .into_iter()
        .filter_map(|n| n.into_string().ok())
        .collect();
    let read = |name: &str| {
        names
            .iter()
            .any(|n| n == name)
            .then(|| skwir_core::fs::xattr::get_deref(path, name).ok().flatten())
            .flatten()
    };
    let mut out = Vec::new();
    if let Some(acl) = read(ACL_ACCESS).as_deref().and_then(acl) {
        out.push(("Access list", acl));
    }
    if let Some(acl) = read(ACL_DEFAULT).as_deref().and_then(acl) {
        out.push(("Default for new items", acl));
    }
    if let Some(caps) = read(CAPABILITIES).as_deref().and_then(capabilities) {
        out.push(("Capabilities", caps));
    }
    if let Some(label) = read(SELINUX) {
        let label = String::from_utf8_lossy(&label)
            .trim_end_matches('\0')
            .trim()
            .to_owned();
        if !label.is_empty() {
            out.push(("SELinux", label));
        }
    }
    out
}

fn rwx(perm: u16) -> String {
    [(4, 'r'), (2, 'w'), (1, 'x')]
        .iter()
        .map(|(bit, c)| if perm & bit != 0 { *c } else { '-' })
        .collect()
}

/// An ACL as the kernel stores it (a version, then tag, permissions and
/// id for each entry), in words: the users and groups it names, and the
/// mask that bounds them. The owner, group and others it repeats from
/// the mode are left out: the Access row says them.
fn acl(value: &[u8]) -> Option<String> {
    const VERSION: u32 = 2;
    const USER: u16 = 0x02;
    const GROUP: u16 = 0x08;
    const MASK: u16 = 0x10;
    const USER_OBJ: u16 = 0x01;
    const GROUP_OBJ: u16 = 0x04;
    const OTHER: u16 = 0x20;
    let (version, entries) = value.split_first_chunk::<4>()?;
    if u32::from_le_bytes(*version) != VERSION || entries.len() % 8 != 0 {
        return None;
    }
    let mut named = Vec::new();
    let mut mask = None;
    let mut base = Vec::new();
    for [t0, t1, p0, p1, i0, i1, i2, i3] in entries.as_chunks::<8>().0.iter().copied() {
        let tag = u16::from_le_bytes([t0, t1]);
        let perm = u16::from_le_bytes([p0, p1]);
        let id = u64::from(u32::from_le_bytes([i0, i1, i2, i3]));
        match tag {
            USER => named.push(format!(
                "user {}: {}",
                crate::users::user(id).unwrap_or_else(|| id.to_string()),
                rwx(perm)
            )),
            GROUP => named.push(format!(
                "group {}: {}",
                crate::users::group(id).unwrap_or_else(|| id.to_string()),
                rwx(perm)
            )),
            MASK => mask = Some(rwx(perm)),
            USER_OBJ => base.push(format!("owner: {}", rwx(perm))),
            GROUP_OBJ => base.push(format!("group: {}", rwx(perm))),
            OTHER => base.push(format!("others: {}", rwx(perm))),
            _ => return None,
        }
    }
    // A default list has no mode to repeat: it is said in full.
    if named.is_empty() {
        return (!base.is_empty()).then(|| base.join(", "));
    }
    if let Some(mask) = mask {
        named.push(format!("at most {mask}"));
    }
    Some(named.join(", "))
}

/// The capabilities' names, by bit.
const CAPS: [&str; 41] = [
    "chown",
    "dac_override",
    "dac_read_search",
    "fowner",
    "fsetid",
    "kill",
    "setgid",
    "setuid",
    "setpcap",
    "linux_immutable",
    "net_bind_service",
    "net_broadcast",
    "net_admin",
    "net_raw",
    "ipc_lock",
    "ipc_owner",
    "sys_module",
    "sys_rawio",
    "sys_chroot",
    "sys_ptrace",
    "sys_pacct",
    "sys_admin",
    "sys_boot",
    "sys_nice",
    "sys_resource",
    "sys_time",
    "sys_tty_config",
    "mknod",
    "lease",
    "audit_write",
    "audit_control",
    "setfcap",
    "mac_override",
    "mac_admin",
    "syslog",
    "wake_alarm",
    "block_suspend",
    "audit_read",
    "perfmon",
    "bpf",
    "checkpoint_restore",
];

fn cap_names(bits: u64) -> String {
    (0..64)
        .filter(|bit| bits & (1 << bit) != 0)
        .map(|bit| match CAPS.get(bit) {
            Some(name) => format!("cap_{name}"),
            None => format!("cap_{bit}"),
        })
        .collect::<Vec<_>>()
        .join(",")
}

/// File capabilities as the kernel stores them (`vfs_cap_data`: a
/// revision and the effective flag, then permitted and inheritable sets),
/// written as `getcap` does: `cap_net_raw=ep`.
fn capabilities(value: &[u8]) -> Option<String> {
    const REVISION_MASK: u32 = 0xFF00_0000;
    const REVISION_1: u32 = 0x0100_0000;
    const EFFECTIVE: u32 = 0x0000_0001;
    let word = |i: usize| -> Option<u32> {
        let bytes = value.get(i * 4..i * 4 + 4)?;
        Some(u32::from_le_bytes(bytes.try_into().ok()?))
    };
    let magic = word(0)?;
    let halves = if magic & REVISION_MASK == REVISION_1 {
        1
    } else {
        2
    };
    let (mut permitted, mut inheritable) = (0u64, 0u64);
    for half in 0..halves {
        permitted |= u64::from(word(1 + half * 2)?) << (32 * half);
        inheritable |= u64::from(word(2 + half * 2)?) << (32 * half);
    }
    let effective = if magic & EFFECTIVE != 0 { "e" } else { "" };
    Some(match (permitted, inheritable) {
        (0, 0) => return None,
        (p, 0) => format!("{}={effective}p", cap_names(p)),
        (0, i) => format!("{}=i", cap_names(i)),
        (p, i) if p == i => format!("{}={effective}ip", cap_names(p)),
        (p, i) => format!("{}={effective}p {}+i", cap_names(p), cap_names(i)),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn acl_bytes(entries: &[(u16, u16, u32)]) -> Vec<u8> {
        let mut out = 2u32.to_le_bytes().to_vec();
        for (tag, perm, id) in entries {
            out.extend(tag.to_le_bytes());
            out.extend(perm.to_le_bytes());
            out.extend(id.to_le_bytes());
        }
        out
    }

    const NONE: u32 = u32::MAX;

    #[test]
    fn an_acl_names_its_users_and_groups_and_its_mask() {
        let access = acl_bytes(&[
            (0x01, 6, NONE),
            (0x02, 6, 4_000_000_001),
            (0x04, 4, NONE),
            (0x08, 5, 4_000_000_002),
            (0x10, 7, NONE),
            (0x20, 4, NONE),
        ]);
        assert_eq!(
            acl(&access).unwrap(),
            "user 4000000001: rw-, group 4000000002: r-x, at most rwx"
        );
        // A default list is said in full.
        let default = acl_bytes(&[(0x01, 7, NONE), (0x04, 5, NONE), (0x20, 0, NONE)]);
        assert_eq!(
            acl(&default).unwrap(),
            "owner: rwx, group: r-x, others: ---"
        );
        assert_eq!(acl(&[1, 0, 0, 0]), None, "another version");
        assert_eq!(acl(&acl_bytes(&[(0x40, 7, 0)])), None, "an unknown tag");
        assert_eq!(acl(&[2, 0, 0, 0, 1]), None, "a truncated entry");
    }

    #[test]
    fn capabilities_read_as_getcap_writes_them() {
        let caps = |magic: u32, words: &[u32]| {
            let mut out = magic.to_le_bytes().to_vec();
            for w in words {
                out.extend(w.to_le_bytes());
            }
            out
        };
        // Revision 2, effective, cap_net_bind_service and cap_net_raw.
        let bind_raw = (1 << 10) | (1 << 13);
        assert_eq!(
            capabilities(&caps(0x0200_0001, &[bind_raw, 0, 0, 0])).unwrap(),
            "cap_net_bind_service,cap_net_raw=ep"
        );
        // Revision 3 (a root id follows), not effective, a high bit.
        assert_eq!(
            capabilities(&caps(0x0300_0000, &[0, 0, 1 << 7, 0, 0])).unwrap(),
            "cap_bpf=p"
        );
        assert_eq!(
            capabilities(&caps(0x0200_0001, &[1, 1, 0, 0])).unwrap(),
            "cap_chown=eip"
        );
        assert_eq!(
            capabilities(&caps(0x0100_0000, &[1 << 21, 1 << 5])).unwrap(),
            "cap_sys_admin=p cap_kill+i"
        );
        assert_eq!(capabilities(&caps(0x0200_0000, &[0, 0, 0, 0])), None);
        assert_eq!(capabilities(&[0, 0]), None);
    }

    #[test]
    fn an_acl_set_on_a_real_file_is_read_back() {
        let base = dirs::home_dir().unwrap().join(".cache").join("skwir-tests");
        std::fs::create_dir_all(&base).unwrap();
        let tmp = tempfile::Builder::new()
            .prefix("access-")
            .tempdir_in(base)
            .unwrap();
        let file = tmp.path().join("it's shared.txt");
        std::fs::write(&file, b"x").unwrap();
        assert!(facts(&file).is_empty(), "a mode alone says nothing more");
        // What `setfacl -m u:4000000001:r` writes: the owner may set it.
        let list = acl_bytes(&[
            (0x01, 6, NONE),
            (0x02, 4, 4_000_000_001),
            (0x04, 4, NONE),
            (0x10, 4, NONE),
            (0x20, 4, NONE),
        ]);
        if skwir_core::fs::xattr::set(&file, ACL_ACCESS, &list).is_err() {
            eprintln!("skipped: no POSIX ACLs here");
            return;
        }
        assert_eq!(
            facts(&file),
            [(
                "Access list",
                "user 4000000001: r--, at most r--".to_owned()
            )]
        );
    }
}
