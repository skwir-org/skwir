//! A path as the user types it: one reading, for the window that completes
//! it and says what it finds there (the address field, a
//! [`Kind::Save`](super::Kind::Save) field) and for whoever is answered
//! with it (Print…). Two readings of one text are a note describing one
//! folder and a file written in another.
//!
//! - A folder's name ends at `/`, on Windows at `\` too; the text splits
//!   at its last one ([`split`]).
//! - `~` starts from the home folder, `.` and `..` are taken as a shell's
//!   `cd` takes them, by the names written, whatever a link among them
//!   leads to ([`resolve`]).
//! - A Save field holds a folder part and a name: `~` alone is the home
//!   folder and no name ([`save_split`]), its folder part names a folder
//!   from the top or from `~` ([`save_folder`]), and a name goes after a
//!   folder part with a separator between them, whatever it ends with
//!   ([`save_join`]).

use std::path::{Component, Path, PathBuf};

/// Whether `c` ends a folder's name in a path typed.
pub fn is_separator(c: char) -> bool {
    c == '/' || (cfg!(windows) && c == '\\')
}

/// The separator a folder is followed by in `text`: the one it already
/// uses, else the system's.
pub fn separator_of(text: &str) -> char {
    text.chars()
        .rev()
        .find(|c| is_separator(*c))
        .unwrap_or(std::path::MAIN_SEPARATOR)
}

/// A path typed, split at its last separator: the folder part, with that
/// separator, kept as typed; the name typed after it.
pub fn split(text: &str) -> (&str, &str) {
    match text.char_indices().rev().find(|(_, c)| is_separator(*c)) {
        Some((at, c)) => text.split_at(at + c.len_utf8()),
        None => ("", text),
    }
}

/// The path `text` names: `~` the home folder, a relative path from
/// `base`, `.` and `..` taken as a shell's `cd` takes them. `None` for a
/// relative path with no `base`, or `~` with no `home`.
pub fn resolve(text: &str, base: Option<&Path>, home: Option<&Path>) -> Option<PathBuf> {
    let tilde = text
        .strip_prefix('~')
        .filter(|rest| rest.is_empty() || rest.starts_with(is_separator));
    let path = if let Some(rest) = tilde {
        home?.join(rest.trim_start_matches(is_separator))
    } else {
        let path = PathBuf::from(text);
        if path.has_root() {
            path
        } else {
            base?.join(path)
        }
    };
    let mut out = PathBuf::new();
    for component in path.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => {
                if matches!(out.components().next_back(), Some(Component::Normal(_))) {
                    out.pop();
                }
            }
            other => out.push(other),
        }
    }
    Some(out)
}

/// A Save field's `text`, split: its folder part and its name; `~` alone
/// is the home folder.
pub fn save_split(text: &str) -> (&str, &str) {
    if text == "~" { (text, "") } else { split(text) }
}

/// The folder a Save field's folder part names, from the top or from `~`:
/// `None` without one, or for a relative one (or `~` with no `home`).
pub fn save_folder(folder: &str, home: Option<&Path>) -> Option<PathBuf> {
    if folder.is_empty() {
        return None;
    }
    resolve(folder, None, home).filter(|path| path.is_absolute())
}

/// `name` after the folder part `folder`: with a separator between them
/// where `folder` does not end with one (`~`, a folder's own name), and
/// the name alone without a folder.
pub fn save_join(folder: &str, name: &str) -> String {
    let mut joined = folder.to_owned();
    if !folder.is_empty() && !name.is_empty() && !folder.ends_with(is_separator) {
        joined.push(separator_of(folder));
    }
    joined.push_str(name);
    joined
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A name goes after any folder part with one separator: `~` alone
    /// is a folder, read back as that folder and that name.
    #[test]
    fn a_name_goes_after_a_folder_with_one_separator() {
        let sep = std::path::MAIN_SEPARATOR;
        assert_eq!(save_join("~", "notes.pdf"), format!("~{sep}notes.pdf"));
        assert_eq!(
            save_join("~/l'été/", "ma liste.pdf"),
            "~/l'été/ma liste.pdf"
        );
        assert_eq!(save_join("/example/a b", "x.pdf"), "/example/a b/x.pdf");
        assert_eq!(save_join("", "notes.pdf"), "notes.pdf");
        assert_eq!(save_join("~", ""), "~");
        assert_eq!(save_join("~/a/", ""), "~/a/");
        assert_eq!(save_split("~"), ("~", ""));
        let joined = save_join("~", "notes.pdf");
        let (folder, name) = save_split(&joined);
        assert_eq!(name, "notes.pdf");
        let home = Path::new("/example/home");
        assert_eq!(save_folder(folder, Some(home)), Some(home.to_owned()));
    }

    /// A Save field's folder is whole or from `~`; `..` is taken off by
    /// the names written, as the address field takes it.
    #[cfg(unix)]
    #[test]
    fn a_save_field_s_folder_is_read_as_cd_reads_it() {
        let home = Path::new("/example/home");
        let folder = |text| save_folder(text, Some(home));
        assert_eq!(folder("~"), Some(PathBuf::from("/example/home")));
        assert_eq!(folder("~/"), Some(PathBuf::from("/example/home")));
        assert_eq!(
            folder("~/link/../b/"),
            Some(PathBuf::from("/example/home/b"))
        );
        assert_eq!(folder("/a b/./c/../d/"), Some(PathBuf::from("/a b/d")));
        assert_eq!(folder(""), None, "no folder part");
        assert_eq!(folder("letters/"), None, "relative");
        assert_eq!(folder("~letters/"), None, "no one's home");
        assert_eq!(save_folder("~/x/", None), None, "no home folder");
    }
}
