//! Tiles from a server: OpenStreetMap's once the user chose it, or one the
//! user named. Only when asked, only the tiles in view, as the
//! OpenStreetMap Foundation's tile policy says (ADR-020): a User-Agent that
//! names bibli, tiles kept as long as their headers say (a week when they
//! say nothing) in the one deletable cache, a conditional request for a
//! tile kept too long, and no offline download.
//!
//! A cached tile is read on a scheduler worker; a download happens on this
//! module's thread (`http`: HTTP/2 where the server speaks it), newest
//! request first and `IN_FLIGHT` at a time, so waiting for a server never
//! holds a worker, and a tile that left the view before its turn is never
//! asked for. A tile downloaded is kept, whatever became of its view.
use std::collections::{HashMap, VecDeque};
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use bibli_core::CancelToken;
use bibli_core::cache::CacheSlot;
use bibli_core::tiles::{TileDone, TileRequest};
use bibli_platform::secrets::{SecretRef, SecretText};
use parking_lot::Mutex;

use crate::http;
use crate::server::{Server, redact};

/// Requests in flight at once: on HTTP/2, streams of one connection; on
/// HTTP/1.1, as many kept-alive connections, as a browser opens.
const IN_FLIGHT: usize = 6;
/// How long a tile is kept when its server says nothing (the policy's
/// minimum).
const KEEP: Duration = Duration::from_hours(168);
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
    /// When the map asked for it (`Downloads::ticket`): the newest is
    /// served first, whatever order the jobs reached the queue in.
    ticket: u64,
    server: Arc<Server>,
    tile: TileRequest,
    /// Where the tile is kept in the cache (`Server::cache_key`).
    key: String,
    stale: Option<Cached>,
    cancel: CancelToken,
    done: TileDone,
}

/// API keys read from the keyring, by server, until the settings change:
/// the keyring is asked once, which may be once the user unlocked it.
type Keys = Mutex<HashMap<String, Option<Arc<SecretText>>>>;

#[derive(Default)]
struct Queue {
    jobs: Mutex<VecDeque<Job>>,
    ready: tokio::sync::Notify,
    started: Mutex<bool>,
}

impl Queue {
    /// The newest request still wanted.
    fn take(&self) -> Option<Job> {
        let mut jobs = self.jobs.lock();
        jobs.retain(|job| !job.cancel.is_cancelled());
        // The view the user is looking at now, and in it the middle (the
        // map asks for it last).
        let newest = (0..jobs.len()).max_by_key(|&i| jobs[i].ticket)?;
        jobs.remove(newest)
    }
}

/// What the download thread shares with its requests.
struct Shared {
    queue: Arc<Queue>,
    http: http::Http,
    cache: CacheSlot,
    stores: Mutex<u32>,
    keys: Arc<Keys>,
}

pub struct Downloads {
    queue: Arc<Queue>,
    cache: CacheSlot,
    /// Ends the thread when the plugin is disabled.
    lifetime: CancelToken,
    user_agent: String,
    keys: Arc<Keys>,
    tickets: std::sync::atomic::AtomicU64,
}

impl Downloads {
    pub fn new(cache: CacheSlot, lifetime: CancelToken) -> Downloads {
        Downloads {
            queue: Arc::default(),
            cache,
            lifetime,
            keys: Arc::default(),
            tickets: std::sync::atomic::AtomicU64::new(0),
            user_agent: format!(
                "bibli/{} (+{})",
                env!("CARGO_PKG_VERSION"),
                env!("CARGO_PKG_REPOSITORY")
            ),
        }
    }

    /// A ticket for a request, taken when the map makes it: the scheduler
    /// runs its jobs newest first too, so the order jobs reach the queue in
    /// is not the order the map asked in.
    pub fn ticket(&self) -> u64 {
        self.tickets
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed)
    }

    /// Get `tile` of `server`: from the cache while it is fresh, else from
    /// the server. Runs on a scheduler worker.
    pub fn get(
        &self,
        ticket: u64,
        server: Arc<Server>,
        tile: TileRequest,
        cancel: CancelToken,
        done: TileDone,
    ) {
        let key = server.cache_key(&tile);
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
        self.enqueue(Job {
            ticket,
            server,
            tile,
            key,
            stale: cached,
            cancel,
            done,
        });
    }

    /// Ask `server` for `tile` now, whatever the cache holds: a server's
    /// Test.
    pub fn ask(&self, server: Arc<Server>, tile: TileRequest, cancel: CancelToken, done: TileDone) {
        let key = server.cache_key(&tile);
        self.enqueue(Job {
            ticket: self.ticket(),
            server,
            tile,
            key,
            stale: None,
            cancel,
            done,
        });
    }

    fn enqueue(&self, job: Job) {
        if let Err(error) = self.start() {
            (job.done)(Err(error));
            return;
        }
        self.queue.jobs.lock().push_back(job);
        self.queue.ready.notify_one();
    }

    /// The settings changed: a key may have too.
    pub fn forget_keys(&self) {
        self.keys.lock().clear();
    }

    /// The download thread, started with the first download.
    fn start(&self) -> anyhow::Result<()> {
        let mut started = self.queue.started.lock();
        if *started {
            return Ok(());
        }
        let shared = Arc::new(Shared {
            queue: Arc::clone(&self.queue),
            http: http::Http::system(&self.user_agent)?,
            cache: self.cache.clone(),
            stores: Mutex::new(0),
            keys: Arc::clone(&self.keys),
        });
        let lifetime = self.lifetime.clone();
        std::thread::Builder::new()
            .name("map-tiles".into())
            .spawn(move || run(&shared, &lifetime))?;
        *started = true;
        Ok(())
    }

    /// Wake the thread so it sees the plugin is gone, and drop what waits:
    /// its requests are cancelled with it.
    pub fn stop(&self) {
        self.lifetime.cancel();
        self.queue.jobs.lock().clear();
        self.queue.ready.notify_one();
    }
}

/// The API key of `server`, from the keyring the first time.
fn key_of(server: &Server, keys: &Keys) -> anyhow::Result<Option<Arc<SecretText>>> {
    if !server.needs_key() {
        return Ok(None);
    }
    if let Some(key) = keys.lock().get(&server.id) {
        return Ok(key.clone());
    }
    let key = bibli_platform::secrets::lookup(SecretRef {
        setting: crate::SERVERS_KEY,
        record: &server.id,
        field: crate::KEY_FIELD,
    })?
    .map(Arc::new);
    keys.lock().insert(server.id.clone(), key.clone());
    Ok(key)
}

/// The download thread: a runtime of its own, serving the newest request
/// first, `IN_FLIGHT` at a time, until the plugin is disabled.
fn run(shared: &Arc<Shared>, lifetime: &CancelToken) {
    let runtime = match tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
    {
        Ok(runtime) => runtime,
        Err(error) => {
            tracing::warn!(%error, "map tiles: no runtime for downloads");
            return;
        }
    };
    runtime.block_on(async {
        let slots = Arc::new(tokio::sync::Semaphore::new(IN_FLIGHT));
        loop {
            let Ok(slot) = Arc::clone(&slots).acquire_owned().await else {
                return;
            };
            let job = loop {
                if lifetime.is_cancelled() {
                    return;
                }
                if let Some(job) = shared.queue.take() {
                    break job;
                }
                // Woken by a new request or by `stop`; the timeout only
                // bounds how long a missed wake could go unnoticed.
                let _ =
                    tokio::time::timeout(Duration::from_secs(30), shared.queue.ready.notified())
                        .await;
            };
            let shared = Arc::clone(shared);
            tokio::spawn(async move {
                serve(&shared, job).await;
                drop(slot);
            });
        }
    });
}

/// Download one tile, keep it, and answer it.
async fn serve(shared: &Arc<Shared>, job: Job) {
    // Apart: the answer (`done`) is sent across threads, never shared, so
    // nothing awaited may hold the job.
    let Job {
        server,
        tile,
        key: place,
        stale,
        cancel,
        done,
        ..
    } = job;
    // The keyring may ask to be unlocked: not on the runtime's thread.
    let (server_for_key, keys) = (Arc::clone(&server), Arc::clone(&shared.keys));
    let key = tokio::task::spawn_blocking(move || key_of(&server_for_key, &keys))
        .await
        .map_err(|error| anyhow::anyhow!("{error}"))
        .and_then(|key| key);
    let secret = key
        .as_ref()
        .ok()
        .and_then(Option::as_ref)
        .map(|k| k.expose().to_owned());
    let result = match &key {
        Ok(_) => {
            fetch(
                &shared.http,
                &server,
                &tile,
                stale.as_ref(),
                secret.as_deref(),
            )
            .await
        }
        Err(error) => Err(anyhow::anyhow!("{error:#}")),
    }
    // Whatever the server or the network said, never the key.
    .map_err(|error| anyhow::anyhow!(redact(&format!("{error:#}"), secret.as_deref())));
    // Decoding and keeping are for a worker, not the runtime's thread.
    let shared = Arc::clone(shared);
    let _ = tokio::task::spawn_blocking(move || {
        // A tile downloaded is kept, even when its view has moved on: the
        // request was made, and the view may come back.
        let answer = result.and_then(|fresh| {
            let image = decode(&fresh.body);
            if image.is_ok() && shared.cache.put(place.as_bytes(), &fresh.encode()).is_ok() {
                let mut count = shared.stores.lock();
                *count += 1;
                if (*count).is_multiple_of(TRIM_EVERY)
                    && let Err(error) = shared.cache.trim_to_bytes(CACHE_BUDGET)
                {
                    tracing::debug!(%error, "map tiles: trimming the cache");
                }
            }
            image
        });
        if !cancel.is_cancelled() {
            done(answer);
        }
    })
    .await;
}

/// Ask the server, conditionally when a stale copy is kept.
async fn fetch(
    http: &http::Http,
    server: &Server,
    tile: &TileRequest,
    stale: Option<&Cached>,
    key: Option<&str>,
) -> anyhow::Result<Cached> {
    let url = server.address(tile, key)?;
    let mut headers = server.request_headers(key)?;
    if let Some(stale) = stale {
        if let Some(etag) = &stale.etag {
            headers.push(("If-None-Match".into(), etag.clone()));
        }
        if let Some(modified) = &stale.modified {
            headers.push(("If-Modified-Since".into(), modified.clone()));
        }
    }
    let answer = http.get(&url, &headers).await?;
    tracing::trace!(version = ?answer.version, status = answer.status, "map tiles: answered");
    let header = |name: &str| {
        answer
            .headers
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
    match answer.status {
        304 => {
            let stale = stale.ok_or_else(|| {
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
            body: answer.body,
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
