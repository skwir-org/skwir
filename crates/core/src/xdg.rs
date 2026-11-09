//! The XDG Base Directory specification's directories, read one way for
//! every caller: a variable unset or empty takes its default, and a
//! relative path in it is ignored, as the specification requires.
use std::ffi::OsString;
use std::path::PathBuf;

/// Absolute paths of a `:`-separated variable, or of its default.
fn list(name: &str, default: &str) -> Vec<PathBuf> {
    split(std::env::var_os(name), default)
}

/// The absolute paths of `value`, or of `default` when it is unset or empty.
fn split(value: Option<OsString>, default: &str) -> Vec<PathBuf> {
    let value = value
        .filter(|v| !v.is_empty())
        .unwrap_or_else(|| OsString::from(default));
    std::env::split_paths(&value)
        .filter(|p| p.is_absolute())
        .collect()
}

/// The folder a variable of the specification holds: an absolute path;
/// unset, empty or relative, none.
fn folder_of(value: Option<OsString>) -> Option<PathBuf> {
    value.map(PathBuf::from).filter(|p| p.is_absolute())
}

/// An absolute path in a variable, or `~` joined to `fallback`.
fn home(name: &str, fallback: &str) -> Option<PathBuf> {
    folder_of(std::env::var_os(name)).or_else(|| crate::os_path::home().map(|h| h.join(fallback)))
}

/// The variables of the specification that name one folder (no list).
const FOLDERS: [&str; 5] = [
    "XDG_DATA_HOME",
    "XDG_CONFIG_HOME",
    "XDG_STATE_HOME",
    "XDG_CACHE_HOME",
    "XDG_RUNTIME_DIR",
];

/// The folder `name` says as a program started from the user's terminal
/// reads it: the session's value, else what the login shell exports
/// (`environment`), read as every other is. `None` for what the shell does
/// not set, and for a name that is no folder of the specification.
///
/// For what names other programs' files (the exclusions' rules, a trash
/// they use). Skwir's own folders are this module's other functions, the
/// session's alone: chosen once as it starts, before the shell has
/// answered, they never move under it.
pub fn in_the_shell(name: &str) -> Option<PathBuf> {
    FOLDERS
        .contains(&name)
        .then(|| folder_of(crate::environment::var(name)))?
}

/// Whether `name` is a variable of the specification that names one folder:
/// read through [`in_the_shell`], never as a list.
pub fn names_a_folder(name: &str) -> bool {
    FOLDERS.contains(&name)
}

/// `$XDG_STATE_HOME`, what a program keeps between runs that is neither
/// configuration nor data worth carrying elsewhere (`~/.local/state`).
pub fn state_home() -> Option<PathBuf> {
    home("XDG_STATE_HOME", ".local/state")
}

/// `$XDG_DATA_HOME`, the user's data (`~/.local/share`).
pub fn data_home() -> Option<PathBuf> {
    home("XDG_DATA_HOME", ".local/share")
}

/// `$XDG_DATA_DIRS`, the system's data (`/usr/local/share:/usr/share`).
pub fn data_dirs() -> Vec<PathBuf> {
    list("XDG_DATA_DIRS", "/usr/local/share:/usr/share")
}

/// Where data is looked for, the user's first: `data_home`, then
/// `data_dirs`.
pub fn data_search() -> Vec<PathBuf> {
    data_home().into_iter().chain(data_dirs()).collect()
}

/// `$XDG_CONFIG_HOME`, the user's configuration (`~/.config`).
pub fn config_home() -> Option<PathBuf> {
    home("XDG_CONFIG_HOME", ".config")
}

/// `$XDG_CACHE_HOME`, what can be made again (`~/.cache`).
pub fn cache_home() -> Option<PathBuf> {
    home("XDG_CACHE_HOME", ".cache")
}

/// `$XDG_RUNTIME_DIR`, the user's private files for this session, which
/// has no default.
pub fn runtime_dir() -> Option<PathBuf> {
    std::env::var_os("XDG_RUNTIME_DIR")
        .map(PathBuf::from)
        .filter(|p| p.is_absolute())
}

/// `$XDG_CONFIG_DIRS`, the system's configuration (`/etc/xdg`).
pub fn config_dirs() -> Vec<PathBuf> {
    list("XDG_CONFIG_DIRS", "/etc/xdg")
}

// Unix's path lists (`:`-separated, `/`-rooted).
#[cfg(all(test, unix))]
mod tests {
    use super::*;

    // Unix's path lists (`:`-separated, `/`-rooted).

    #[cfg(unix)]
    #[test]
    fn empty_takes_the_default_and_relative_paths_are_ignored() {
        let paths = |value: Option<&str>| split(value.map(OsString::from), "/default");
        assert_eq!(paths(None), [PathBuf::from("/default")]);
        assert_eq!(paths(Some("")), [PathBuf::from("/default")]);
        assert_eq!(paths(Some("relative:/abs::")), [PathBuf::from("/abs")]);
    }

    #[test]
    fn a_folder_is_a_whole_path_and_no_list() {
        let folder = |value: &str| folder_of(Some(OsString::from(value)));
        assert_eq!(folder(""), None);
        assert_eq!(folder("relative/x"), None);
        assert_eq!(folder("/a:/b"), Some(PathBuf::from("/a:/b")));
        assert_eq!(folder_of(None), None);
        assert!(names_a_folder("XDG_DATA_HOME"));
        assert!(!names_a_folder("XDG_DATA_DIRS"), "a list");
        assert_eq!(in_the_shell("CARGO_HOME"), None, "no folder of the XDG's");
    }
}
