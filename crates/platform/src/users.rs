//! Names for the numeric owner and group of a file.
//!
//! Read once from `/etc/passwd` and `/etc/group` by `load`, on a worker: no
//! unsafe `getpwuid`, and no lookup service that could block. A user
//! those files do not list (LDAP, a container's stray id) keeps its number.
use std::collections::HashMap;
use std::sync::OnceLock;

fn table(path: &str) -> HashMap<u64, String> {
    std::fs::read_to_string(path)
        .map(|text| parse(&text))
        .unwrap_or_default()
}

/// `name:password:id:...` lines; the first name of an id wins.
pub(crate) fn parse(text: &str) -> HashMap<u64, String> {
    let mut names = HashMap::new();
    for line in text.lines() {
        let mut fields = line.split(':');
        if let (Some(name), Some(_), Some(id)) = (fields.next(), fields.next(), fields.next())
            && let Ok(id) = id.parse()
            && !name.is_empty()
        {
            names.entry(id).or_insert_with(|| name.to_owned());
        }
    }
    names
}

static USERS: OnceLock<HashMap<u64, String>> = OnceLock::new();
static GROUPS: OnceLock<HashMap<u64, String>> = OnceLock::new();

/// Read both files. Call it from a worker: a file read can block.
pub fn load() {
    USERS.get_or_init(|| table("/etc/passwd"));
    GROUPS.get_or_init(|| table("/etc/group"));
}

/// Whether `load` has finished: names shown before it did are only numbers.
pub fn loaded() -> bool {
    USERS.get().is_some() && GROUPS.get().is_some()
}

/// The user's name, once `load` has run; never reads a file itself, so it is
/// safe where blocking is not allowed.
pub fn user(uid: u64) -> Option<String> {
    USERS.get()?.get(&uid).cloned()
}

pub fn group(gid: u64) -> Option<String> {
    GROUPS.get()?.get(&gid).cloned()
}

/// `rwxr-xr-x (755)` for the permission bits of a mode.
pub fn mode(mode: u64) -> String {
    let bits = mode & 0o7777;
    let mut text = String::with_capacity(9);
    for shift in [6, 3, 0] {
        let triplet = (bits >> shift) & 0o7;
        text.push(if triplet & 4 != 0 { 'r' } else { '-' });
        text.push(if triplet & 2 != 0 { 'w' } else { '-' });
        text.push(if triplet & 1 != 0 { 'x' } else { '-' });
    }
    format!("{text} ({bits:o})")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_and_modes_read_as_words() {
        let names = parse(
            "root:x:0:0:root:/root:/bin/sh\n# comment\nada:x:1000:100::/home/ada:/bin/sh\nalias:x:1000:100::/:/bin/sh\n",
        );
        assert_eq!(names.get(&0).map(String::as_str), Some("root"));
        assert_eq!(
            names.get(&1000).map(String::as_str),
            Some("ada"),
            "the first name wins"
        );
        assert_eq!(names.len(), 2);
        assert_eq!(mode(0o100_644), "rw-r--r-- (644)");
        assert_eq!(mode(0o040_755), "rwxr-xr-x (755)");
    }
}
