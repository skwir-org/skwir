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

/// An absolute path in a variable, or `~` joined to `fallback`.
fn home(name: &str, fallback: &str) -> Option<PathBuf> {
    std::env::var_os(name)
        .map(PathBuf::from)
        .filter(|p| p.is_absolute())
        .or_else(|| dirs::home_dir().map(|h| h.join(fallback)))
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

/// `$XDG_CONFIG_DIRS`, the system's configuration (`/etc/xdg`).
pub fn config_dirs() -> Vec<PathBuf> {
    list("XDG_CONFIG_DIRS", "/etc/xdg")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_takes_the_default_and_relative_paths_are_ignored() {
        let paths = |value: Option<&str>| split(value.map(OsString::from), "/default");
        assert_eq!(paths(None), [PathBuf::from("/default")]);
        assert_eq!(paths(Some("")), [PathBuf::from("/default")]);
        assert_eq!(paths(Some("relative:/abs::")), [PathBuf::from("/abs")]);
    }
}
