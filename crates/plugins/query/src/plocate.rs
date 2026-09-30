//! Use the system's `plocate` database for name-only queries over `/`, when
//! it is installed: it answers in milliseconds what a walk takes minutes for.
//!
//! The database knows nothing of our options, so its answer is a superset
//! that the caller still matches: we only drop what `hidden:` and `depth:`
//! exclude, which a walk would never have visited.

use std::ffi::OsString;
use std::os::unix::ffi::OsStringExt;
use std::path::{Component, Path, PathBuf};
use std::process::{Command, Stdio};

use crate::lang::Query;

/// Candidate paths from `plocate`, or `None` when it does not apply: not
/// installed, root is not `/`, the query is not an AND of name substrings,
/// or the database could not answer (the walk is used instead).
pub fn try_locate(q: &Query) -> Option<Vec<PathBuf>> {
    if q.root != Path::new("/") {
        return None;
    }
    let patterns = q.name_substrings()?;
    let bin = bibli_core::os_path::find_program("plocate")?;
    let paths = locate_with(&bin, &patterns, q.case_sensitive)?;
    Some(
        paths
            .into_iter()
            .filter(|p| visible(p, q.hidden, q.depth))
            .collect(),
    )
}

/// Whether a walk of `/` with these options would reach `path`: absolute,
/// or relative to where the walk starts.
pub(crate) fn visible(path: &Path, hidden: bool, depth: Option<usize>) -> bool {
    let mut names = 0;
    for c in path.components() {
        match c {
            Component::Normal(name) => {
                names += 1;
                if !hidden && name.as_encoded_bytes().starts_with(b".") {
                    return false;
                }
            }
            Component::RootDir => {}
            // plocate prints absolute, normalized paths; anything else is not ours.
            _ => return false,
        }
    }
    names > 0 && depth.is_none_or(|d| names <= d)
}

/// Run `bin` with plocate's flags and parse NUL-separated output. `None`
/// when plocate failed rather than found nothing.
pub fn locate_with(bin: &Path, patterns: &[String], case_sensitive: bool) -> Option<Vec<PathBuf>> {
    let mut cmd = Command::new(bin);
    cmd.arg("--basename").arg("--null");
    if !case_sensitive {
        cmd.arg("--ignore-case");
    }
    cmd.arg("--")
        .args(patterns)
        .stdin(Stdio::null())
        .stderr(Stdio::piped());
    let out = cmd.output().ok()?;
    if !out.status.success() {
        // plocate exits 1, silently, when nothing matched. A message (a
        // missing or unreadable database) means it could not answer.
        return (out.status.code() == Some(1) && out.stdout.is_empty() && out.stderr.is_empty())
            .then(Vec::new);
    }
    Some(
        out.stdout
            .split(|b| *b == 0)
            .filter(|s| !s.is_empty())
            .map(|s| PathBuf::from(OsString::from_vec(s.to_vec())))
            .collect(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hidden_and_depth_are_what_a_walk_would_skip() {
        assert!(visible(Path::new("/a/b.txt"), false, None));
        assert!(!visible(Path::new("/a/.git/b.txt"), false, None));
        assert!(!visible(Path::new("/.b"), false, None));
        assert!(visible(Path::new("/a/.git/b.txt"), true, None));
        assert!(visible(Path::new("/a/b"), false, Some(2)));
        assert!(!visible(Path::new("/a/b/c"), false, Some(2)));
        assert!(!visible(Path::new("/"), true, None));
    }
}
