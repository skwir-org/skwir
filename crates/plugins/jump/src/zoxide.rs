//! zoxide as a frecency source: `zoxide query --list --score`, cached briefly.

use std::path::PathBuf;
use std::process::Command;
use std::time::{Duration, Instant};

use parking_lot::Mutex;

/// A directory zoxide knows, with its score.
#[derive(Clone, Debug, PartialEq)]
pub struct ZEntry {
    pub score: f64,
    pub path: PathBuf,
}

/// Parse `zoxide query --list --score` output (`  12.5 /path with spaces`).
pub fn parse(output: &str) -> Vec<ZEntry> {
    output
        .lines()
        .filter_map(|l| {
            let l = l.trim_start();
            let (score, path) = l.split_once(' ')?;
            let score: f64 = score.parse().ok()?;
            let path = path.trim_start();
            (!path.is_empty()).then(|| ZEntry {
                score,
                path: PathBuf::from(path),
            })
        })
        .collect()
}

/// Run zoxide if it is on PATH. Missing binary or failure yields an empty list.
pub fn query() -> Vec<ZEntry> {
    let Ok(out) = Command::new("zoxide")
        .args(["query", "--list", "--score"])
        .output()
    else {
        return Vec::new();
    };
    if !out.status.success() {
        return Vec::new();
    }
    parse(&String::from_utf8_lossy(&out.stdout))
}

/// Caches the zoxide list for a short while.
#[derive(Default)]
pub struct ZoxideCache {
    inner: Mutex<Option<(Instant, Vec<ZEntry>)>>,
}

impl ZoxideCache {
    pub const TTL: Duration = Duration::from_secs(10);

    pub fn get(&self) -> Vec<ZEntry> {
        let mut g = self.inner.lock();
        if let Some((at, v)) = g.as_ref()
            && at.elapsed() < Self::TTL
        {
            return v.clone();
        }
        let v = query();
        *g = Some((Instant::now(), v.clone()));
        v
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_scores_and_paths_with_spaces() {
        let v = parse("  42.5 /home/x/proj\n   3 /tmp/a b\nbad line\n");
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
