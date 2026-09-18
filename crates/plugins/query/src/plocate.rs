//! Use the system's `plocate` database for name-only queries over `/`, when
//! it is installed: it answers in milliseconds what a walk takes minutes for.

use std::os::unix::ffi::OsStringExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use crate::lang::Query;

fn on_path(bin: &str) -> Option<PathBuf> {
    std::env::var_os("PATH").and_then(|p| {
        std::env::split_paths(&p)
            .map(|d| d.join(bin))
            .find(|p| p.is_file())
    })
}

/// Candidate paths from `plocate`, or `None` when it does not apply
/// (not installed, root is not `/`, query is not name-only, no substrings).
pub fn try_locate(q: &Query) -> Option<Vec<PathBuf>> {
    if q.root != Path::new("/") || !q.is_name_only() {
        return None;
    }
    let patterns = q.name_substrings();
    if patterns.is_empty() {
        return None;
    }
    let bin = on_path("plocate")?;
    locate_with(&bin, &patterns, q.case_sensitive)
}

/// Run `bin` with plocate's flags and parse NUL-separated output.
pub fn locate_with(bin: &Path, patterns: &[String], case_sensitive: bool) -> Option<Vec<PathBuf>> {
    let mut cmd = Command::new(bin);
    cmd.arg("--basename").arg("--null");
    if !case_sensitive {
        cmd.arg("--ignore-case");
    }
    cmd.arg("--")
        .args(patterns)
        .stdin(Stdio::null())
        .stderr(Stdio::null());
    let out = cmd.output().ok()?;
    if !out.status.success() && out.stdout.is_empty() {
        // plocate exits 1 when nothing matched; treat as an empty result set.
        return Some(Vec::new());
    }
    Some(
        out.stdout
            .split(|b| *b == 0)
            .filter(|s| !s.is_empty())
            .map(|s| PathBuf::from(std::ffi::OsString::from_vec(s.to_vec())))
            .collect(),
    )
}
