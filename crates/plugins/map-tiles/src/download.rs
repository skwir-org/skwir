//! Tiles from a server: OpenStreetMap's once the user chose it, or one the
//! user named. Only when asked, only the tiles in view, as the
//! OpenStreetMap Foundation's tile policy says (ADR-020): a User-Agent that
//! names bibli, tiles kept as long as their headers say (a week when they
//! say nothing) in the one deletable cache, a conditional request for a
//! tile kept too long, and no offline download.
//!
//! A cached tile is read on a scheduler worker; a download happens on this
//! module's two threads, newest request first, so waiting for a server
//! never holds a worker, and a tile that left the view before its turn is
//! never asked for.
use std::collections::VecDeque;
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use bibli_core::CancelToken;
use bibli_core::cache::CacheSlot;
use bibli_core::tiles::TileDone;
use parking_lot::{Condvar, Mutex};

/// Connections to one server at a time.
const THREADS: usize = 2;
/// How long a tile is kept when its server says nothing (the policy's
/// minimum).
const KEEP: Duration = Duration::from_secs(7 * 24 * 3600);
/// A tile larger than this is not a tile.
const MAX_BODY: u64 = 4 * 1024 * 1024;
/// The downloaded tiles kept, across sources.
pub const CACHE_BUDGET: u64 = 128 * 1024 * 1024;
/// Stores between two trims of the cache: a trim walks the directory.
const TRIM_EVERY: u32 = 64;

/// A tile as the cache keeps it: what the server said, then its bytes.
struct Cached {
    expires: u64,
    etag: Option<String>,
    modified: Option<String>,
    body: Vec<u8>,
}

fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_secs())
}

impl Cached {
    fn encode(&self) -> Vec<u8> {
        let head = serde_json::json!({
            "expires": self.expires,
            "etag": self.etag,
            "modified": self.modified,
        })
        .to_string();
        let mut out = Vec::with_capacity(head.len() + 1 + self.body.len());
        out.extend(head.as_bytes());
        out.push(b'\n');
        out.extend(&self.body);
        out
    }

    fn decode(bytes: &[u8]) -> Option<Cached> {
        let split = bytes.iter().position(|&b| b == b'\n')?;
        let head: serde_json::Value = serde_json::from_slice(&bytes[..split]).ok()?;
        let text = |key| head.get(key).and_then(|v| v.as_str()).map(str::to_owned);
        Some(Cached {
            expires: head.get("expires")?.as_u64()?,
            etag: text("etag"),
            modified: text("modified"),
            body: bytes[split + 1..].to_vec(),
        })
    }
}

/// When a response may be used until, from its headers: `max-age`, else
/// `Expires`, never less than a week.
fn expiry(cache_control: Option<&str>, expires: Option<&str>, now: u64) -> u64 {
    let max_age = cache_control.and_then(|value| {
        value.split(',').find_map(|part| {
            part.trim()
                .strip_prefix("max-age=")
                .and_then(|n| n.trim().parse::<u64>().ok())
        })
    });
    let until = max_age
        .map(|age| now + age)
        .or_else(|| expires.and_then(httpdate).map(|t| t.max(now)));
    until.unwrap_or(0).max(now + KEEP.as_secs())
}

/// An HTTP date (`Sun, 06 Nov 1994 08:49:37 GMT`), as seconds since 1970.
fn httpdate(text: &str) -> Option<u64> {
    let parts: Vec<&str> = text.split_whitespace().collect();
    let [_, day, month, year, time, "GMT"] = parts.as_slice() else {
        return None;
    };
    let month = [
        "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
    ]
    .iter()
    .position(|m| m == month)? as u32
        + 1;
    let days =
        bibli_platform::calendar::days_from_civil(year.parse().ok()?, month, day.parse().ok()?);
    let mut hms = time.split(':').map(str::parse::<u64>);
    let (h, m, s) = (hms.next()?.ok()?, hms.next()?.ok()?, hms.next()?.ok()?);
    u64::try_from(days)
        .ok()
        .map(|d| d * 86_400 + h * 3600 + m * 60 + s)
}

struct Job {
    url: String,
    key: String,
    stale: Option<Cached>,
    cancel: CancelToken,
    done: TileDone,
}

#[derive(Default)]
struct Queue {
    jobs: Mutex<VecDeque<Job>>,
    ready: Condvar,
    /// Threads started so far.
    started: Mutex<usize>,
}

pub struct Downloads {
    queue: Arc<Queue>,
    cache: CacheSlot,
    stores: Arc<Mutex<u32>>,
    /// Ends the threads when the plugin is disabled.
    lifetime: CancelToken,
    user_agent: String,
}

impl Downloads {
    pub fn new(cache: CacheSlot, lifetime: CancelToken) -> Downloads {
        Downloads {
            queue: Arc::default(),
            cache,
            stores: Arc::default(),
            lifetime,
            user_agent: format!(
                "bibli/{} (+{})",
                env!("CARGO_PKG_VERSION"),
                env!("CARGO_PKG_REPOSITORY")
            ),
        }
    }

    /// Get the tile at `url`: from the cache while it is fresh, else from
    /// its server. Runs on a scheduler worker.
    pub fn get(&self, url: String, cancel: CancelToken, done: TileDone) {
        let key = url.clone();
        let cached = self
            .cache
            .get(key.as_bytes())
            .and_then(|bytes| Cached::decode(&bytes));
        if let Some(cached) = cached.as_ref().filter(|c| c.expires > now()) {
            if !cancel.is_cancelled() {
                done(decode(&cached.body));
            }
            return;
        }
        self.start_threads();
        self.queue.jobs.lock().push_back(Job {
            url,
            key,
            stale: cached,
            cancel,
            done,
        });
        self.queue.ready.notify_one();
    }

    fn start_threads(&self) {
        let mut started = self.queue.started.lock();
        while *started < THREADS {
            *started += 1;
            let queue = Arc::clone(&self.queue);
            let cache = self.cache.clone();
            let stores = Arc::clone(&self.stores);
            let lifetime = self.lifetime.clone();
            let agent: ureq::Agent = ureq::Agent::config_builder()
                .user_agent(self.user_agent.as_str())
                .timeout_global(Some(Duration::from_secs(15)))
                .http_status_as_error(false)
                .tls_config(
                    ureq::tls::TlsConfig::builder()
                        .root_certs(ureq::tls::RootCerts::PlatformVerifier)
                        .build(),
                )
                .build()
                .into();
            let spawned = std::thread::Builder::new()
                .name("map-tiles".into())
                .spawn(move || work(&queue, &agent, &cache, &stores, &lifetime));
            if let Err(error) = spawned {
                tracing::warn!(%error, "map tiles: no download thread");
                *started -= 1;
                break;
            }
        }
    }

    /// Wake the threads so they see the plugin is gone, and drop what
    /// waits: its requests are cancelled with it.
    pub fn stop(&self) {
        self.lifetime.cancel();
        self.queue.jobs.lock().clear();
        self.queue.ready.notify_all();
    }
}

fn work(
    queue: &Queue,
    agent: &ureq::Agent,
    cache: &CacheSlot,
    stores: &Mutex<u32>,
    lifetime: &CancelToken,
) {
    loop {
        let job = {
            let mut jobs = queue.jobs.lock();
            loop {
                if lifetime.is_cancelled() {
                    return;
                }
                // The newest first: the view the user is looking at now.
                if let Some(job) = jobs.pop_back() {
                    if job.cancel.is_cancelled() {
                        continue;
                    }
                    break job;
                }
                queue.ready.wait_for(&mut jobs, Duration::from_secs(30));
            }
        };
        let result = fetch(agent, &job);
        if job.cancel.is_cancelled() {
            continue;
        }
        let answer = result.and_then(|fresh| {
            let image = decode(&fresh.body);
            if image.is_ok() && cache.put(job.key.as_bytes(), &fresh.encode()).is_ok() {
                let mut count = stores.lock();
                *count += 1;
                if (*count).is_multiple_of(TRIM_EVERY)
                    && let Err(error) = cache.trim_to_bytes(CACHE_BUDGET)
                {
                    tracing::debug!(%error, "map tiles: trimming the cache");
                }
            }
            image
        });
        (job.done)(answer);
    }
}

/// Ask the server, once more when the connection failed: a kept-alive
/// connection the server closed while idle fails the first request sent
/// on it.
fn fetch(agent: &ureq::Agent, job: &Job) -> anyhow::Result<Cached> {
    match fetch_once(agent, job) {
        Err(error) if error.is::<ureq::Error>() && !job.cancel.is_cancelled() => {
            tracing::debug!(%error, "map tiles: asking again");
            fetch_once(agent, job)
        }
        result => result,
    }
}

/// Ask the server, conditionally when a stale copy is kept.
fn fetch_once(agent: &ureq::Agent, job: &Job) -> anyhow::Result<Cached> {
    let mut request = agent.get(&job.url);
    if let Some(stale) = &job.stale {
        if let Some(etag) = &stale.etag {
            request = request.header("If-None-Match", etag);
        }
        if let Some(modified) = &stale.modified {
            request = request.header("If-Modified-Since", modified);
        }
    }
    let mut response = request.call()?;
    let header = |name: &str| {
        response
            .headers()
            .get(name)
            .and_then(|v| v.to_str().ok())
            .map(str::to_owned)
    };
    let expires = expiry(
        header("cache-control").as_deref(),
        header("expires").as_deref(),
        now(),
    );
    let (etag, modified) = (header("etag"), header("last-modified"));
    match response.status().as_u16() {
        304 => {
            let stale = job.stale.as_ref().ok_or_else(|| {
                anyhow::anyhow!("the server said nothing changed in a tile never had")
            })?;
            Ok(Cached {
                expires,
                etag: etag.or_else(|| stale.etag.clone()),
                modified: modified.or_else(|| stale.modified.clone()),
                body: stale.body.clone(),
            })
        }
        200 => Ok(Cached {
            expires,
            etag,
            modified,
            body: response
                .body_mut()
                .with_config()
                .limit(MAX_BODY)
                .read_to_vec()?,
        }),
        status => anyhow::bail!("the tile server answered {status}"),
    }
}

fn decode(body: &[u8]) -> anyhow::Result<bibli_core::thumbs::Image> {
    bibli_platform::inspection::image(body)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_tile_is_kept_as_long_as_its_server_says_and_at_least_a_week() {
        let now = 1_000_000;
        let week = KEEP.as_secs();
        assert_eq!(expiry(None, None, now), now + week);
        assert_eq!(expiry(Some("public, max-age=3600"), None, now), now + week);
        assert_eq!(expiry(Some("max-age=2000000"), None, now), now + 2_000_000);
        let later = httpdate("Sun, 06 Nov 2044 08:49:37 GMT").unwrap();
        assert_eq!(
            expiry(None, Some("Sun, 06 Nov 2044 08:49:37 GMT"), now),
            later
        );
        assert_eq!(httpdate("Sun, 06 Nov 1994 08:49:37 GMT"), Some(784_111_777));
        assert!(httpdate("yesterday").is_none());
    }

    #[test]
    fn a_cached_tile_reads_back() {
        let tile = Cached {
            expires: 42,
            etag: Some("\"abc\"".into()),
            modified: None,
            body: b"\x89PNG\n\x00bytes".to_vec(),
        };
        let back = Cached::decode(&tile.encode()).unwrap();
        assert_eq!(back.expires, 42);
        assert_eq!(back.etag.as_deref(), Some("\"abc\""));
        assert_eq!(back.body, tile.body);
    }
}
