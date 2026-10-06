//! Raw filesystem measurements: the numbers the core's streaming listing
//! must approach. All timings are warm-cache (dropping caches needs root).
//! The raw listing and stat are the system's own: getdents and statx on
//! Linux, readdir and fstatat on other Unix systems, `std::fs` elsewhere.

use std::collections::BTreeMap;
use std::path::Path;
use std::time::{Duration, Instant};

use bibli_core::os_path::from_bytes_ref;
use rayon::prelude::*;
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
    /// The tags plugin's provider over every entry of the directory (tags,
    /// rating, comment and origins from the user attributes), as hydration
    /// and a search run it.
    pub tags_fill: Stat,
    /// A `tag:` search of the whole tree through the query plugin, until
    /// `Complete`.
    pub tag_search: Stat,
    /// The tags of the whole tree checked again (F5 in its tag location),
    /// counted before and unchanged since: the check of what changed,
    /// until it ends.
    pub tags_check: Stat,
    /// The tags index (`bibli_plugin_tags::index`) learning 500 000
    /// tagged files, in batches as a count teaches it.
    pub tags_index_learn: Stat,
    /// Then the counts under the home folder, as a tag location asks.
    pub tags_index_tally: Stat,
    /// Then one file retagged and the index written, as after an edit.
    pub tags_index_change: Stat,
}

/// Budgets file: `[profile] metric_us = max`.
#[derive(Debug, Deserialize)]
pub struct Budgets {
    #[serde(flatten)]
    pub profiles: BTreeMap<String, BTreeMap<String, u64>>,
}

impl Stat {
    /// Nothing measured: a metric that does not apply to the profile.
    const NONE: Stat = Stat {
        min_us: 0,
        median_us: 0,
    };

    /// Min and median of `samples`; `NONE` for no samples.
    fn of(mut samples: Vec<Duration>) -> Stat {
        samples.sort();
        let us = |d: &Duration| u64::try_from(d.as_micros()).unwrap_or(u64::MAX);
        match (samples.first(), samples.get(samples.len() / 2)) {
            (Some(min), Some(median)) => Stat {
                min_us: us(min),
                median_us: us(median),
            },
            _ => Stat::NONE,
        }
    }
}

fn timed<T>(runs: usize, mut f: impl FnMut() -> T) -> Stat {
    Stat::of(
        (0..runs)
            .map(|_| {
                let t = Instant::now();
                std::hint::black_box(f());
                t.elapsed()
            })
            .collect(),
    )
}

#[cfg(unix)]
fn open_dir(path: &Path) -> rustix::io::Result<rustix::fd::OwnedFd> {
    use rustix::fs::{Mode, OFlags};
    rustix::fs::open(
        path,
        OFlags::RDONLY | OFlags::DIRECTORY | OFlags::CLOEXEC,
        Mode::empty(),
    )
}

#[cfg(unix)]
fn getdents_first_batch(path: &Path) -> usize {
    let fd = open_dir(path).expect("open dir");
    let dir = rustix::fs::Dir::read_from(&fd).expect("getdents");
    dir.take(1024).count()
}

#[cfg(not(unix))]
fn getdents_first_batch(path: &Path) -> usize {
    std::fs::read_dir(path)
        .expect("read_dir")
        .take(1024)
        .count()
}

#[cfg(unix)]
fn getdents_full(path: &Path) -> usize {
    let fd = open_dir(path).expect("open dir");
    let dir = rustix::fs::Dir::read_from(&fd).expect("getdents");
    dir.filter_map(Result::ok)
        .map(|e| (e.file_type(), e.file_name().to_bytes().len()))
        .fold(0, |n, e| {
            std::hint::black_box(e);
            n + 1
        })
}

#[cfg(not(unix))]
fn getdents_full(path: &Path) -> usize {
    std_full(path)
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

/// The names in `path`, as `OsStr::as_encoded_bytes` gives them.
fn names(path: &Path) -> Vec<Vec<u8>> {
    std::fs::read_dir(path)
        .expect("read_dir")
        .filter_map(Result::ok)
        .map(|e| e.file_name().as_encoded_bytes().to_vec())
        .collect()
}

#[cfg(target_os = "linux")]
fn statx_all(path: &Path, names: &[Vec<u8>]) -> u64 {
    use rustix::fs::{AtFlags, StatxFlags};
    let fd = open_dir(path).expect("open dir");
    names
        .iter()
        .filter_map(|n| {
            rustix::fs::statx(
                &fd,
                &*from_bytes_ref(n),
                AtFlags::SYMLINK_NOFOLLOW,
                StatxFlags::BASIC_STATS,
            )
            .ok()
        })
        .map(|s| s.stx_size)
        .sum()
}

#[cfg(all(unix, not(target_os = "linux")))]
fn statx_all(path: &Path, names: &[Vec<u8>]) -> u64 {
    let fd = open_dir(path).expect("open dir");
    names
        .iter()
        .filter_map(|n| {
            rustix::fs::statat(
                &fd,
                &*from_bytes_ref(n),
                rustix::fs::AtFlags::SYMLINK_NOFOLLOW,
            )
            .ok()
        })
        .map(|s| u64::try_from(s.st_size).unwrap_or(0))
        .sum()
}

#[cfg(not(unix))]
fn statx_all(path: &Path, names: &[Vec<u8>]) -> u64 {
    names
        .iter()
        .filter_map(|n| std::fs::symlink_metadata(path.join(&*from_bytes_ref(n))).ok())
        .map(|m| m.len())
        .sum()
}

fn stat_parallel(path: &Path, names: &[Vec<u8>]) -> u64 {
    names
        .par_iter()
        .filter_map(|n| std::fs::symlink_metadata(path.join(&*from_bytes_ref(n))).ok())
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

/// The longest a listing may take before the bench gives up on it: far
/// beyond any budget, short of hanging CI.
const STREAM_DEADLINE: Duration = Duration::from_secs(120);

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
        let left = STREAM_DEADLINE.saturating_sub(t.elapsed());
        match rx
            .recv_timeout(left)
            .unwrap_or_else(|e| panic!("listing {} did not complete: {e}", path.display()))
        {
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

/// Request thumbnails for `paths` through a fresh app with the providers
/// the thumbnails plugin registers for images, the cache at `cache`; return
/// the time until every one is `Ready`, divided by the count.
fn thumbs_pass(paths: &[std::path::PathBuf], cache: &Path, scratch: &Path) -> Duration {
    use bibli_core::scheduler::Priority;
    use bibli_core::thumbs::{ThumbRequest, ThumbSize, ThumbState};
    use bibli_platform::thumb_providers::{FreedesktopCache, ImageDecoder};
    use bibli_platform::thumbnails::ThumbCache;
    let app = bibli_core::App::new(bibli_core::app::AppConfig::ephemeral(scratch)).expect("app");
    app.thumbnails.register(
        "bench",
        std::sync::Arc::new(FreedesktopCache::at(ThumbCache::at(cache), true)),
    );
    app.thumbnails
        .register("bench", std::sync::Arc::new(ImageDecoder));
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

/// Cold and warm thumbnail timings over the first 200 images of `dir`, each
/// run with a fresh thumbnail cache beside the trees, on the same disk
/// (a cache on tmpfs would make write-back look free).
fn thumbs(root: &Path, dir: &Path, runs: usize) -> (Stat, Stat) {
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
        .map(|p| bibli_core::os_path::canonical(p).expect("canonical"))
        .collect();
    let mut cold = Vec::new();
    let mut warm = Vec::new();
    for _ in 0..runs {
        let scratch = tempfile::Builder::new()
            .prefix(".thumbs-")
            .tempdir_in(root)
            .expect("scratch dir");
        let cache = scratch.path().join("thumbnails");
        cold.push(thumbs_pass(&paths, &cache, scratch.path()));
        warm.push(thumbs_pass(&paths, &cache, scratch.path()));
    }
    (Stat::of(cold), Stat::of(warm))
}

/// An app with the tags plugin and every other notes store's (ADR-027),
/// as the application runs them, and the query plugin to search with it.
pub fn tags_app() -> std::sync::Arc<bibli_core::App> {
    let app = bibli_core::App::new(bibli_core::app::AppConfig::ephemeral(
        std::env::temp_dir().join("bibli-bench-tags"),
    ))
    .expect("app");
    app.plugins.add(
        std::sync::Arc::new(bibli_plugin_tags::TagsPlugin::default()),
        &app,
    );
    app.plugins
        .add(std::sync::Arc::new(bibli_plugin_xmp::XmpPlugin), &app);
    app.plugins.add(
        std::sync::Arc::new(bibli_plugin_tagspaces::TagSpacesPlugin),
        &app,
    );
    app.plugins.add(
        std::sync::Arc::new(bibli_plugin_name_tags::NameTagsPlugin),
        &app,
    );
    app.plugins.add(
        std::sync::Arc::new(bibli_plugin_query::QueryPlugin::default()),
        &app,
    );
    app
}

/// The metadata providers over every entry of `dir`: stat-ed once, then
/// filled afresh each run.
fn tags_fill(app: &bibli_core::App, dir: &Path, runs: usize) -> Stat {
    let entries: Vec<bibli_core::Entry> = std::fs::read_dir(dir)
        .expect("read dir")
        .flatten()
        .filter_map(|e| bibli_core::fs::stat_path(&e.path()))
        .collect();
    Stat::of(
        (0..runs)
            .map(|_| {
                let mut batch = entries.clone();
                let t = Instant::now();
                app.metadata.fill(dir, &mut batch);
                let took = t.elapsed();
                std::hint::black_box(batch);
                took
            })
            .collect(),
    )
}

/// A search measured: until its first file, until `Complete`, and how
/// many it found.
pub struct Searched {
    pub first: Option<Duration>,
    pub complete: Duration,
    pub found: usize,
}

/// Search `expr` under `root` through the query plugin.
pub fn search(app: &bibli_core::App, expr: &str, root: &Path) -> Searched {
    use bibli_core::location::ListingEvent;
    let t = Instant::now();
    let location = bibli_plugin_query::location_for(expr, Some(root));
    let open = app.open_location(&location).expect("open search");
    let mut found = 0;
    let mut first = None;
    loop {
        let left = STREAM_DEADLINE.saturating_sub(t.elapsed());
        match open
            .events()
            .recv_timeout(left)
            .unwrap_or_else(|e| panic!("search {expr} did not complete: {e}"))
        {
            ListingEvent::Added(batch) => {
                first.get_or_insert_with(|| t.elapsed());
                found += batch.len();
            }
            ListingEvent::Complete => break,
            ListingEvent::Error(e) => panic!("search error: {e}"),
            _ => {}
        }
    }
    let complete = t.elapsed();
    open.close();
    Searched {
        first,
        complete,
        found,
    }
}

/// `tag:` over the whole of `profile`'s tree, when the generator could tag
/// it: only the deep tree, `NONE` otherwise. Measured once every other
/// measurement is done: reading a million files' attributes leaves the
/// kernel reclaiming for a moment, which would slow what comes next.
pub fn tag_search(root: &Path, profile: Profile, runs: usize) -> Stat {
    let dir = root.join(profile.name());
    if profile != Profile::Deep1m || !tagged(&dir) {
        return Stat::NONE;
    }
    let tags = tags_app();
    let root = bibli_core::os_path::canonical(&dir).expect("tree");
    let expr = format!("tag:{}", crate::generate::TAG);
    Stat::of(
        (0..runs)
            .map(|_| {
                let searched = search(&tags, &expr, &root);
                assert_eq!(searched.found, 1000, "one file in a thousand is tagged");
                searched.complete
            })
            .collect(),
    )
}

/// The tags of the whole tree browsed as folders (ADR-032): counted once in
/// full, then checked again `runs` times (F5), each check finding nothing
/// changed, timed until it ends.
pub fn tags_check(root: &Path, profile: Profile, runs: usize) -> Stat {
    use bibli_core::command::{Invocation, Outcome};
    let dir = root.join(profile.name());
    if profile != Profile::Deep1m || !tagged(&dir) {
        return Stat::NONE;
    }
    let app = tags_app();
    let root = bibli_core::os_path::canonical(&dir).expect("tree");
    let pane = app.workspace.read().active_id();
    let call = |name: &str, args: serde_json::Value| {
        app.dispatch(&Invocation::with_args(name, &args))
            .expect(name)
    };
    let counted = || {
        let Outcome::Value(v) = call(
            "tags.counted",
            serde_json::json!({ "root": bibli_core::os_path::PathArg::from(root.as_path()) }),
        ) else {
            panic!("tags.counted answers a value");
        };
        v
    };
    let location = bibli_plugin_tags::location::Address {
        scope: Some(root.clone()),
        ..Default::default()
    }
    .location();
    app.navigate(pane, &location).expect("navigate");
    // Its listing completes once the full count ends.
    assert!(app.pump_until_complete(pane, Duration::from_secs(600)));
    assert!(
        counted()["counted"].is_u64(),
        "a full count covers the tree"
    );
    Stat::of(
        (0..runs)
            .map(|_| {
                let t = Instant::now();
                call("refresh", serde_json::json!({ "pane": pane }));
                while counted()["counting"] == true {
                    app.pump();
                    std::thread::sleep(Duration::from_millis(1));
                }
                t.elapsed()
            })
            .collect(),
    )
}

/// The tags index at the scale of a large library: 500 000 tagged files
/// in 20 000 folders under a home folder, learned, counted, changed. The
/// index writes to a cache of its own, emptied first.
pub fn tags_index(profile: Profile, runs: usize) -> (Stat, Stat, Stat) {
    use bibli_plugin_tags::index::TagIndex;
    if profile != Profile::Deep1m {
        return (Stat::NONE, Stat::NONE, Stat::NONE);
    }
    let app = tags_app();
    let home = std::env::home_dir().expect("home");
    let words = [
        "vacation", "alps", "work", "family", "2024", "todo", "receipts", "kids",
    ];
    let file = |i: usize| {
        home.join(format!(
            "Pictures/{}/{}/IMG_{i:06}.jpg",
            2000 + i % 25,
            i % 800
        ))
    };
    let tags = |i: usize| -> Vec<String> {
        (0..=(i % 3))
            .map(|k| words[(i + k * 3) % 8].to_owned())
            .collect()
    };
    let (mut learn, mut tally, mut change) = (Vec::new(), Vec::new(), Vec::new());
    for run in 0..runs {
        // A cache of its own each run: the last run's index may still be
        // writing to its own.
        let cache = std::env::temp_dir().join(format!("bibli-bench-tags-index-{run}"));
        let _ = std::fs::remove_dir_all(&cache);
        let slot = bibli_core::cache::Cache::new(&cache).namespace("tags");
        let index = TagIndex::new(Some(slot), &app, bibli_core::CancelToken::new());
        let t = Instant::now();
        for batch in (0..500_000).collect::<Vec<usize>>().chunks(2_000) {
            index.learn(batch.iter().map(|&i| (file(i), tags(i))).collect());
        }
        learn.push(t.elapsed());
        let t = Instant::now();
        let (counted, files) = index.tally(&home);
        tally.push(t.elapsed());
        assert_eq!((counted.len(), files), (8, 500_000));
        index.write_now();
        let t = Instant::now();
        index.learn(vec![(file(run), vec!["changed".into()])]);
        index.write_now();
        change.push(t.elapsed());
    }
    (Stat::of(learn), Stat::of(tally), Stat::of(change))
}

/// Whether the generator could tag `dir` (see `generate::tag`).
fn tagged(dir: &Path) -> bool {
    dir.join(".bibli-bench-tagged").exists()
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
    let (thumbs_cold_per_image, thumbs_warm_per_image) = if profile == Profile::Images10k {
        thumbs(root, &dir, runs)
    } else {
        (Stat::NONE, Stat::NONE)
    };
    let tags = tags_app();
    let tags_fill = match profile {
        Profile::Deep1m => Stat::NONE,
        _ => tags_fill(&tags, &single, runs),
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
        stream_open: Stat::of(opens),
        stream_first_batch: Stat::of(firsts),
        stream_complete: Stat::of(completes),
        thumbs_cold_per_image,
        thumbs_warm_per_image,
        tags_fill,
        // Measured after every profile (`tag_search`, `tags_check`).
        tag_search: Stat::NONE,
        tags_check: Stat::NONE,
        tags_index_learn: Stat::NONE,
        tags_index_tally: Stat::NONE,
        tags_index_change: Stat::NONE,
    }
}

/// Compare `report` against `budgets`; prints one line per metric. Returns false on regression.
pub fn check(profile: Profile, report: &Report, budgets: &Budgets) -> bool {
    let Some(b) = budgets.profiles.get(profile.name()) else {
        eprintln!("bench: {}: no budgets, skipped", profile.name());
        return true;
    };
    let measured: [(&str, &Stat); 17] = [
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
        ("tags_fill_us", &report.tags_fill),
        ("tag_search_us", &report.tag_search),
        ("tags_check_us", &report.tags_check),
        ("tags_index_learn_us", &report.tags_index_learn),
        ("tags_index_tally_us", &report.tags_index_tally),
        ("tags_index_change_us", &report.tags_index_change),
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
