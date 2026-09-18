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
    /// Through `bibli-core`'s path provider: time for `open` to return.
    pub stream_open: Stat,
    /// Through the provider: time until the first `Added` batch is received.
    pub stream_first_batch: Stat,
    /// Through the provider: time until `Complete`.
    pub stream_complete: Stat,
    /// Thumbnail pipeline, per image, with an empty freedesktop cache (decode + write-back).
    pub thumbs_cold_per_image: Stat,
    /// Thumbnail pipeline, per image, served from the freedesktop cache written by the cold run.
    pub thumbs_warm_per_image: Stat,
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

/// Open `path` through the core provider; return (open, first batch, complete) durations.
fn stream(app: &bibli_core::App, path: &Path) -> (Duration, Duration, Duration) {
    use bibli_core::location::{ListingEvent, Location};
    let t = Instant::now();
    let open = app.open_location(&Location::from_path(path)).expect("open");
    let opened = t.elapsed();
    let rx = open.events();
    let mut first = None;
    let complete;
    loop {
        match rx.recv().expect("event") {
            ListingEvent::Added(_) => {
                first.get_or_insert_with(|| t.elapsed());
            }
            ListingEvent::Complete => {
                complete = t.elapsed();
                break;
            }
            ListingEvent::Error(e) => panic!("listing error: {e}"),
            _ => {}
        }
    }
    open.close();
    (opened, first.unwrap_or(complete), complete)
}

/// Request thumbnails for `paths` through a fresh app with the thumbnails
/// plugin; return the time until every one is `Ready`, divided by the count.
fn thumbs_pass(paths: &[std::path::PathBuf], scratch: &Path) -> Duration {
    use bibli_core::scheduler::Priority;
    use bibli_core::thumbs::{ThumbRequest, ThumbSize, ThumbState};
    let app = bibli_core::App::new(bibli_core::app::AppConfig::ephemeral(scratch)).expect("app");
    app.plugins.add(
        std::sync::Arc::new(bibli_plugin_thumbnails::ThumbnailsPlugin),
        &app,
    );
    let reqs: Vec<ThumbRequest> = paths
        .iter()
        .map(|p| ThumbRequest {
            entry: bibli_core::op::EntryRef::from_path(p).expect("stat"),
            mime: Some("image/png".into()),
            mtime: None,
            size: ThumbSize::Normal,
        })
        .collect();
    let t = Instant::now();
    for r in &reqs {
        app.thumbnails.get(r.clone(), Priority::Visible);
    }
    let mut pending: Vec<&ThumbRequest> = reqs.iter().collect();
    while !pending.is_empty() {
        pending.retain(
            |r| match app.thumbnails.get((*r).clone(), Priority::Visible) {
                ThumbState::Ready(_) => false,
                ThumbState::Unavailable => {
                    panic!("thumbnail unavailable: {}", r.entry.path.display())
                }
                ThumbState::Pending => true,
            },
        );
        if !pending.is_empty() {
            std::thread::sleep(Duration::from_micros(200));
        }
    }
    t.elapsed() / u32::try_from(paths.len()).unwrap_or(1)
}

/// Cold and warm thumbnail timings over the first 200 images of `dir`,
/// with `XDG_CACHE_HOME` pointed at a temp dir for the whole process.
#[allow(unsafe_code)]
fn thumbs(dir: &Path, runs: usize) -> (Stat, Stat) {
    let mut paths: Vec<std::path::PathBuf> = std::fs::read_dir(dir)
        .expect("images dir")
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|x| x == "png"))
        .collect();
    paths.sort();
    paths.truncate(200);
    let paths: Vec<_> = paths
        .into_iter()
        .map(|p| std::fs::canonicalize(p).expect("canonical"))
        .collect();
    let mut cold = Vec::new();
    let mut warm = Vec::new();
    for _ in 0..runs {
        let cache = tempfile::tempdir().expect("tempdir");
        // SAFETY: the bench is single-threaded at this point and owns its environment.
        unsafe { std::env::set_var("XDG_CACHE_HOME", cache.path()) };
        cold.push(thumbs_pass(&paths, cache.path()));
        warm.push(thumbs_pass(&paths, cache.path()));
    }
    let stat_of = |mut v: Vec<Duration>| {
        v.sort();
        Stat {
            min_us: v[0].as_micros() as u64,
            median_us: v[v.len() / 2].as_micros() as u64,
        }
    };
    (stat_of(cold), stat_of(warm))
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
    let app = bibli_core::App::new(bibli_core::app::AppConfig::ephemeral(
        std::env::temp_dir().join("bibli-bench"),
    ))
    .expect("app");
    let mut opens = Vec::new();
    let mut firsts = Vec::new();
    let mut completes = Vec::new();
    for _ in 0..runs {
        let (o, f, c) = stream(&app, &single);
        opens.push(o);
        firsts.push(f);
        completes.push(c);
    }
    let stat_of = |mut v: Vec<Duration>| {
        v.sort();
        Stat {
            min_us: v[0].as_micros() as u64,
            median_us: v[v.len() / 2].as_micros() as u64,
        }
    };
    let (thumbs_cold_per_image, thumbs_warm_per_image) = if profile == Profile::Images10k {
        thumbs(&dir, runs)
    } else {
        (
            Stat {
                min_us: 0,
                median_us: 0,
            },
            Stat {
                min_us: 0,
                median_us: 0,
            },
        )
    };
    Report {
        files: profile.file_count(),
        runs,
        first_batch: timed(runs, || getdents_first_batch(&single)),
        list_getdents: timed(runs, || getdents_full(&single)),
        list_std: timed(runs, || std_full(&single)),
        stat_statx: timed(runs, || statx_all(&single, &n)),
        stat_parallel: timed(runs, || stat_parallel(&single, &n)),
        walk_parallel: timed(runs, || walk_parallel(&dir)),
        stream_open: stat_of(opens),
        stream_first_batch: stat_of(firsts),
        stream_complete: stat_of(completes),
        thumbs_cold_per_image,
        thumbs_warm_per_image,
    }
}

/// Compare `report` against `budgets`; prints one line per metric. Returns false on regression.
pub fn check(profile: Profile, report: &Report, budgets: &Budgets) -> bool {
    let Some(b) = budgets.profiles.get(profile.name()) else {
        eprintln!("bench: {}: no budgets, skipped", profile.name());
        return true;
    };
    let measured: [(&str, &Stat); 11] = [
        ("first_batch_us", &report.first_batch),
        ("list_getdents_us", &report.list_getdents),
        ("list_std_us", &report.list_std),
        ("stat_statx_us", &report.stat_statx),
        ("stat_parallel_us", &report.stat_parallel),
        ("walk_parallel_us", &report.walk_parallel),
        ("stream_open_us", &report.stream_open),
        ("stream_first_batch_us", &report.stream_first_batch),
        ("stream_complete_us", &report.stream_complete),
        ("thumbs_cold_per_image_us", &report.thumbs_cold_per_image),
        ("thumbs_warm_per_image_us", &report.thumbs_warm_per_image),
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
