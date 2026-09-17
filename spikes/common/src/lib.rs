//! Shared contract for the toolkit spikes: identical rows, identical lazy
//! thumbnail pipeline, identical scroll script, identical metrics and report.
//! A spike only has to render the rows and call `Metrics::frame` once per frame.

use std::collections::{HashMap, VecDeque};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use serde::Serialize;

// ---------------------------------------------------------------- args

#[derive(Debug, Clone)]
pub struct Args {
    pub rows: usize,
    pub images: PathBuf,
    pub duration_secs: f64,
    pub out: Option<PathBuf>,
}

/// `--rows N --images DIR --duration SECS --out FILE`
pub fn parse_args() -> Args {
    let mut a = Args {
        rows: 100_000,
        images: PathBuf::from("bench-data/images10k"),
        duration_secs: 5.0,
        out: None,
    };
    let mut it = std::env::args().skip(1);
    while let Some(k) = it.next() {
        let v = it.next().unwrap_or_default();
        match k.as_str() {
            "--rows" => a.rows = v.parse().expect("--rows"),
            "--images" => a.images = v.into(),
            "--duration" => a.duration_secs = v.parse().expect("--duration"),
            "--out" => a.out = Some(v.into()),
            _ => panic!("unknown arg {k}"),
        }
    }
    a
}

// ---------------------------------------------------------------- rows

pub const ROW_HEIGHT_PX: f32 = 48.0;
pub const THUMB_PX: f32 = 40.0;

#[derive(Debug, Clone)]
pub struct Row {
    pub name: String,
    pub size: String,
    pub modified: String,
}

const EXTS: [&str; 8] = ["txt", "rs", "png", "pdf", "md", "jpg", "toml", "log"];

/// Deterministic synthetic row, cheap enough to compute per frame.
pub fn row(i: usize) -> Row {
    let size = (i as u64 * 7919) % 5_000_000;
    Row {
        name: format!("file-{i:06}.{}", EXTS[i % EXTS.len()]),
        size: format!("{:.1} KB", size as f64 / 1024.0),
        modified: format!("2026-{:02}-{:02} {:02}:{:02}", 1 + i % 12, 1 + i % 28, i % 24, i % 60),
    }
}

// ---------------------------------------------------------------- thumbnails

/// Decoded RGBA8 image.
pub struct Decoded {
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
}

struct ThumbState {
    cache: HashMap<usize, Arc<Decoded>>,
    order: VecDeque<usize>,
    /// Pending requests, most recent last (workers pop from the back: LIFO
    /// approximates viewport priority).
    queue: Vec<(usize, u64)>,
    in_flight: HashMap<usize, ()>,
    frame: u64,
    decoded: u64,
    skipped: u64,
}

/// Lazy, cancellable thumbnail loader over the images directory.
pub struct Thumbs {
    files: Vec<PathBuf>,
    state: Mutex<ThumbState>,
    cv: std::sync::Condvar,
    on_ready: Mutex<Option<Box<dyn Fn() + Send + Sync>>>,
    cap: usize,
}

impl Thumbs {
    /// Start `workers` decode threads over the PNGs in `dir`.
    pub fn new(dir: &std::path::Path, workers: usize) -> Arc<Self> {
        let mut files: Vec<PathBuf> = std::fs::read_dir(dir)
            .unwrap_or_else(|e| panic!("images dir {}: {e}", dir.display()))
            .filter_map(Result::ok)
            .map(|e| e.path())
            .filter(|p| p.extension().is_some_and(|x| x == "png"))
            .collect();
        files.sort();
        assert!(!files.is_empty(), "no PNGs in {}", dir.display());
        let this = Arc::new(Thumbs {
            files,
            state: Mutex::new(ThumbState {
                cache: HashMap::new(),
                order: VecDeque::new(),
                queue: Vec::new(),
                in_flight: HashMap::new(),
                frame: 0,
                decoded: 0,
                skipped: 0,
            }),
            cv: std::sync::Condvar::new(),
            on_ready: Mutex::new(None),
            cap: 1024,
        });
        for _ in 0..workers {
            let t = Arc::clone(&this);
            std::thread::spawn(move || t.worker());
        }
        this
    }

    /// Called by the toolkit to be woken (request a redraw) when a decode lands.
    pub fn set_on_ready(&self, f: impl Fn() + Send + Sync + 'static) {
        *self.on_ready.lock().unwrap() = Some(Box::new(f));
    }

    /// Call once per frame so stale requests can be dropped.
    pub fn tick(&self) {
        self.state.lock().unwrap().frame += 1;
    }

    /// Image index backing row `i` (rows cycle through the images).
    pub fn image_index(&self, row: usize) -> usize {
        row % self.files.len()
    }

    /// Return the decoded image if present; otherwise enqueue it and return None.
    pub fn get(&self, row: usize) -> Option<Arc<Decoded>> {
        let idx = self.image_index(row);
        let mut s = self.state.lock().unwrap();
        if let Some(d) = s.cache.get(&idx) {
            return Some(Arc::clone(d));
        }
        if !s.in_flight.contains_key(&idx) {
            s.in_flight.insert(idx, ());
            let f = s.frame;
            s.queue.push((idx, f));
            self.cv.notify_one();
        }
        None
    }

    pub fn decoded_count(&self) -> u64 {
        self.state.lock().unwrap().decoded
    }

    pub fn skipped_count(&self) -> u64 {
        self.state.lock().unwrap().skipped
    }

    fn worker(&self) {
        loop {
            let (idx, path) = {
                let mut s = self.state.lock().unwrap();
                loop {
                    if let Some((idx, requested)) = s.queue.pop() {
                        // Requested more than 3 frames ago: scrolled away, skip.
                        if s.frame.saturating_sub(requested) > 3 {
                            s.in_flight.remove(&idx);
                            s.skipped += 1;
                            continue;
                        }
                        break (idx, self.files[idx].clone());
                    }
                    s = self.cv.wait(s).unwrap();
                }
            };
            let img = image::open(&path).expect("decode png").to_rgba8();
            let d = Arc::new(Decoded { width: img.width(), height: img.height(), rgba: img.into_raw() });
            {
                let mut s = self.state.lock().unwrap();
                s.in_flight.remove(&idx);
                s.cache.insert(idx, d);
                s.order.push_back(idx);
                s.decoded += 1;
                while s.order.len() > self.cap {
                    if let Some(old) = s.order.pop_front() {
                        s.cache.remove(&old);
                    }
                }
            }
            if let Some(f) = self.on_ready.lock().unwrap().as_ref() {
                f();
            }
        }
    }
}

// ---------------------------------------------------------------- scroll script

/// Deterministic scroll script: `duration` seconds of continuous scrolling
/// at 3000 px/s, then 2 seconds of random jumps every 250 ms.
pub struct ScrollDriver {
    total_px: f32,
    duration: f64,
    seed: u64,
}

impl ScrollDriver {
    pub fn new(rows: usize, duration_secs: f64) -> Self {
        ScrollDriver { total_px: rows as f32 * ROW_HEIGHT_PX, duration: duration_secs, seed: 0x9E37_79B9_7F4A_7C15 }
    }

    /// Target scroll offset in px for `t` seconds after the first frame.
    pub fn offset_at(&self, t: f64) -> f32 {
        if t < self.duration {
            ((t * 3000.0) as f32) % self.total_px
        } else {
            let step = ((t - self.duration) / 0.25) as u64;
            let mut x = self.seed ^ step.wrapping_mul(0x2545_F491_4F6C_DD1D);
            x ^= x >> 33;
            x = x.wrapping_mul(0xFF51_AFD7_ED55_8CCD);
            x ^= x >> 33;
            (x % 10_000) as f32 / 10_000.0 * self.total_px
        }
    }

    pub fn done(&self, t: f64) -> bool {
        t >= self.duration + 2.0
    }
}

// ---------------------------------------------------------------- metrics

#[derive(Debug, Serialize)]
pub struct Report {
    pub toolkit: String,
    pub gpu: String,
    pub rows: usize,
    pub first_frame_ms: f64,
    pub frames: usize,
    pub frame_p50_us: u64,
    pub frame_p95_us: u64,
    pub frame_p99_us: u64,
    pub frame_max_us: u64,
    pub frames_over_8_3ms: usize,
    pub frames_over_16_7ms: usize,
    pub rss_mb: f64,
    pub thumbnails_decoded: u64,
    pub thumbnails_skipped: u64,
}

pub struct Metrics {
    start: Instant,
    first_frame: Option<Instant>,
    last_frame: Option<Instant>,
    frame_times: Vec<Duration>,
}

impl Metrics {
    /// Call at the very top of `main`, before the toolkit initializes.
    pub fn start() -> Self {
        Metrics { start: Instant::now(), first_frame: None, last_frame: None, frame_times: Vec::with_capacity(4096) }
    }

    /// Call once per rendered frame. Returns seconds since the first frame.
    pub fn frame(&mut self) -> f64 {
        let now = Instant::now();
        if self.first_frame.is_none() {
            self.first_frame = Some(now);
            eprintln!("first frame at {:.1} ms", self.start.elapsed().as_secs_f64() * 1000.0);
        } else if let Some(last) = self.last_frame {
            self.frame_times.push(now - last);
        }
        self.last_frame = Some(now);
        (now - self.first_frame.unwrap()).as_secs_f64()
    }

    pub fn report(&self, toolkit: &str, rows: usize, thumbs: &Thumbs) -> Report {
        let mut ft = self.frame_times.clone();
        ft.sort();
        let pct = |p: f64| ft.get(((ft.len() as f64 * p) as usize).min(ft.len().saturating_sub(1))).map_or(0, |d| d.as_micros() as u64);
        Report {
            toolkit: toolkit.into(),
            gpu: std::env::var("SPIKE_GPU").unwrap_or_else(|_| "unknown".into()),
            rows,
            first_frame_ms: self.first_frame.map_or(0.0, |f| (f - self.start).as_secs_f64() * 1000.0),
            frames: ft.len(),
            frame_p50_us: pct(0.50),
            frame_p95_us: pct(0.95),
            frame_p99_us: pct(0.99),
            frame_max_us: ft.last().map_or(0, |d| d.as_micros() as u64),
            frames_over_8_3ms: ft.iter().filter(|d| d.as_micros() > 8_300).count(),
            frames_over_16_7ms: ft.iter().filter(|d| d.as_micros() > 16_700).count(),
            rss_mb: rss_mb(),
            thumbnails_decoded: thumbs.decoded_count(),
            thumbnails_skipped: thumbs.skipped_count(),
        }
    }
}

/// Resident set size from /proc/self/statm.
pub fn rss_mb() -> f64 {
    let s = std::fs::read_to_string("/proc/self/statm").unwrap_or_default();
    let pages: f64 = s.split_whitespace().nth(1).and_then(|x| x.parse().ok()).unwrap_or(0.0);
    pages * 4096.0 / 1_048_576.0
}

/// Print the report to stdout and write it to `--out` if given, then exit.
pub fn finish(report: &Report, args: &Args) -> ! {
    let json = serde_json::to_string_pretty(report).unwrap();
    println!("{json}");
    if let Some(out) = &args.out {
        std::fs::write(out, &json).expect("write report");
    }
    std::process::exit(0)
}
