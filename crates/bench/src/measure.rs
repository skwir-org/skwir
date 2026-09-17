//! Raw filesystem measurements: the numbers the core's streaming listing
//! must approach. All timings are warm-cache (dropping caches needs root).

use std::collections::BTreeMap;
use std::ffi::OsStr;
use std::os::unix::ffi::OsStrExt;
use std::path::Path;
use std::time::{Duration, Instant};

use rayon::prelude::*;
use rustix::fs::{AtFlags, Dir, Mode, OFlags, StatxFlags};
use serde::{Deserialize, Serialize};

use crate::profiles::Profile;

/// One measured quantity: min and median over the runs, in microseconds.
#[derive(Debug, Serialize)]
pub struct Stat {
    pub min_us: u64,
    pub median_us: u64,
}

/// All measurements for one profile.
#[derive(Debug, Serialize)]
pub struct Report {
    pub files: u64,
    pub runs: usize,
    /// Streaming listing via getdents: time until the first 1024 entries.
    pub first_batch: Stat,
    /// Full listing via getdents, names and `d_type` only, no stat.
    pub list_getdents: Stat,
    /// Full listing via `std::fs::read_dir`, names and file types.
    pub list_std: Stat,
    /// `statx` of every entry through the directory fd, single-threaded.
    pub stat_statx: Stat,
    /// `symlink_metadata` of every entry, rayon-parallel.
    pub stat_parallel: Stat,
    /// Recursive walk counting files, rayon-parallel (only meaningful for deep trees).
    pub walk_parallel: Stat,
}

/// Budgets file: `[profile] metric_us = max`.
#[derive(Debug, Deserialize)]
pub struct Budgets {
    #[serde(flatten)]
    pub profiles: BTreeMap<String, BTreeMap<String, u64>>,
}

fn timed<T>(runs: usize, mut f: impl FnMut() -> T) -> Stat {
    let mut samples: Vec<Duration> = (0..runs)
        .map(|_| {
            let t = Instant::now();
            std::hint::black_box(f());
            t.elapsed()
        })
        .collect();
    samples.sort();
    Stat {
        min_us: samples[0].as_micros() as u64,
        median_us: samples[samples.len() / 2].as_micros() as u64,
    }
}

fn open_dir(path: &Path) -> rustix::io::Result<rustix::fd::OwnedFd> {
    rustix::fs::open(
        path,
        OFlags::RDONLY | OFlags::DIRECTORY | OFlags::CLOEXEC,
        Mode::empty(),
    )
}

fn getdents_first_batch(path: &Path) -> usize {
    let fd = open_dir(path).expect("open dir");
    let dir = Dir::read_from(&fd).expect("getdents");
    dir.take(1024).count()
}

fn getdents_full(path: &Path) -> usize {
    let fd = open_dir(path).expect("open dir");
    let dir = Dir::read_from(&fd).expect("getdents");
    dir.filter_map(Result::ok)
        .map(|e| (e.file_type(), e.file_name().to_bytes().len()))
        .fold(0, |n, e| {
            std::hint::black_box(e);
            n + 1
        })
}

fn std_full(path: &Path) -> usize {
    std::fs::read_dir(path)
        .expect("read_dir")
        .filter_map(Result::ok)
        .map(|e| (e.file_type().ok(), e.file_name()))
        .fold(0, |n, e| {
            std::hint::black_box(e);
            n + 1
        })
}

fn names(path: &Path) -> Vec<Vec<u8>> {
    let fd = open_dir(path).expect("open dir");
    Dir::read_from(&fd)
        .expect("getdents")
        .filter_map(Result::ok)
        .map(|e| e.file_name().to_bytes().to_vec())
        .filter(|n| n != b"." && n != b"..")
        .collect()
}

fn statx_all(path: &Path, names: &[Vec<u8>]) -> u64 {
    let fd = open_dir(path).expect("open dir");
    names
        .iter()
        .filter_map(|n| {
            rustix::fs::statx(
                &fd,
                OsStr::from_bytes(n),
                AtFlags::SYMLINK_NOFOLLOW,
                StatxFlags::BASIC_STATS,
            )
            .ok()
        })
        .map(|s| s.stx_size)
        .sum()
}

fn stat_parallel(path: &Path, names: &[Vec<u8>]) -> u64 {
    names
        .par_iter()
        .filter_map(|n| std::fs::symlink_metadata(path.join(OsStr::from_bytes(n))).ok())
        .map(|m| m.len())
        .sum()
}

fn walk_parallel(path: &Path) -> u64 {
    let mut count = 0u64;
    let mut dirs = vec![path.to_path_buf()];
    while !dirs.is_empty() {
        let (files, next): (Vec<u64>, Vec<Vec<std::path::PathBuf>>) = dirs
            .par_iter()
            .map(|d| {
                let mut f = 0u64;
                let mut sub = Vec::new();
                if let Ok(rd) = std::fs::read_dir(d) {
                    for e in rd.flatten() {
                        match e.file_type() {
                            Ok(t) if t.is_dir() => sub.push(e.path()),
                            Ok(_) => f += 1,
                            Err(_) => {}
                        }
                    }
                }
                (f, sub)
            })
            .unzip();
        count += files.iter().sum::<u64>();
        dirs = next.into_iter().flatten().collect();
    }
    count
}

/// Measure `profile` under `root`.
pub fn run(root: &Path, profile: Profile, runs: usize) -> Report {
    let dir = root.join(profile.name());
    // Walk is over the whole tree; the per-directory measurements target the
    // largest single directory (the root for flat profiles, a leaf for deep).
    let single = match profile {
        Profile::Deep1m => dir.join("d0/d0/d0/d0"),
        _ => dir.clone(),
    };
    let n = names(&single);
    Report {
        files: profile.file_count(),
        runs,
        first_batch: timed(runs, || getdents_first_batch(&single)),
        list_getdents: timed(runs, || getdents_full(&single)),
        list_std: timed(runs, || std_full(&single)),
        stat_statx: timed(runs, || statx_all(&single, &n)),
        stat_parallel: timed(runs, || stat_parallel(&single, &n)),
        walk_parallel: timed(runs, || walk_parallel(&dir)),
    }
}

/// Compare `report` against `budgets`; prints one line per metric. Returns false on regression.
pub fn check(profile: Profile, report: &Report, budgets: &Budgets) -> bool {
    let Some(b) = budgets.profiles.get(profile.name()) else {
        eprintln!("bench: {}: no budgets, skipped", profile.name());
        return true;
    };
    let measured: [(&str, &Stat); 6] = [
        ("first_batch_us", &report.first_batch),
        ("list_getdents_us", &report.list_getdents),
        ("list_std_us", &report.list_std),
        ("stat_statx_us", &report.stat_statx),
        ("stat_parallel_us", &report.stat_parallel),
        ("walk_parallel_us", &report.walk_parallel),
    ];
    let mut ok = true;
    for (key, stat) in measured {
        if let Some(&max) = b.get(key) {
            // Compare the min: the budget is about what the machine can do, not scheduler noise.
            let pass = stat.min_us <= max;
            ok &= pass;
            eprintln!(
                "bench: {}.{key}: {} us (budget {max}) {}",
                profile.name(),
                stat.min_us,
                if pass { "ok" } else { "FAIL" }
            );
        }
    }
    ok
}
