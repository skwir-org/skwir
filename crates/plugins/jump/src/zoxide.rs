//! zoxide as a frecency source: `zoxide query --list --score`.

use std::path::PathBuf;
use std::process::Command;
use std::time::Duration;

use bibli_core::CancelToken;

/// A directory zoxide knows, with its score.
#[derive(Clone, Debug, PartialEq)]
pub struct ZEntry {
    pub score: f64,
    pub path: PathBuf,
}

/// Parse `zoxide query --list --score` output (`  12.5 /path with spaces`).
/// Paths are bytes: a name that is not UTF-8 comes through as it is.
pub fn parse(output: &[u8]) -> Vec<ZEntry> {
    output
        .split(|b| *b == b'\n')
        .filter_map(|line| {
            let line = line.trim_ascii_start();
            let space = line.iter().position(|b| *b == b' ')?;
            let score: f64 = std::str::from_utf8(&line[..space]).ok()?.parse().ok()?;
            let path = line[space..].trim_ascii_start();
            (!path.is_empty()).then(|| ZEntry {
                score,
                path: path_from(path.to_vec()),
            })
        })
        .collect()
}

#[cfg(unix)]
fn path_from(bytes: Vec<u8>) -> PathBuf {
    PathBuf::from(<std::ffi::OsString as std::os::unix::ffi::OsStringExt>::from_vec(bytes))
}

#[cfg(not(unix))]
fn path_from(bytes: Vec<u8>) -> PathBuf {
    String::from_utf8_lossy(&bytes).into_owned().into()
}

/// Ask zoxide, if it is installed. Worker-only: this runs a process. A
/// missing binary, a failure or a timeout yields an empty list.
pub fn query(cancel: &CancelToken) -> Vec<ZEntry> {
    let Some(zoxide) = bibli_core::os_path::find_program("zoxide") else {
        return Vec::new();
    };
    let mut command = Command::new(zoxide);
    command.args(["query", "--list", "--score"]);
    match bibli_platform::process::run(&mut command, cancel, Duration::from_secs(5)) {
        Ok(out) => parse(&out),
        Err(e) => {
            tracing::debug!("zoxide: {e:#}");
            Vec::new()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_scores_and_paths_with_spaces() {
        let v = parse(b"  42.5 /home/x/proj\n   3 /tmp/a b\nbad line\n");
        assert_eq!(v.len(), 2);
        assert_eq!(
            v[0],
            ZEntry {
                score: 42.5,
                path: "/home/x/proj".into()
            }
        );
        assert_eq!(v[1].path, PathBuf::from("/tmp/a b"));
    }
}
