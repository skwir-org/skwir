//! What a mount Skwir made sends to its server after a file is closed
//! (ADR-102): with rclone's `writes` or `full` cache, a file written is
//! kept on this disk and sent a few seconds after its `close`, and tried
//! again while the server refuses it. Asked of the mount's remote control
//! (`vfs/queue`, `options/get`), where it answers on each system
//! ([`control_of`]: a socket of the runtime folder on Unix, the loopback
//! behind its login on Windows), sent now (`vfs/queue-set-expiry`) and
//! waited for, so a copy is done when the server has it ([`RcloneSends`])
//! and a Disconnect loses nothing.
//!
//! A file joins the queue only once rclone has its release, which the
//! kernel sends after `close` returned, on its own time: what a plan wrote
//! is waited for by rclone's record of each file in its cache
//! ([`send_written`]), unsent from its first write until the server has
//! it, never by what waits when asked.

use std::collections::{BTreeMap, HashMap, HashSet};
use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::time::Duration;

use parking_lot::Mutex;
use skwir_core::CancelToken;
use skwir_core::op::Cancelled;
use skwir_core::op::transfer::{Aside, Control, NotServed, Told, Transfer};

use crate::rc::{At, call};

/// Where the remote control of the rclone mount Skwir made at `point`
/// answers: its socket, on Unix; on Windows, the loopback its login was
/// kept for (`winfsp::rc_of`). `None` for a mount Skwir did not make.
pub fn control_of(point: &Path) -> Option<At> {
    #[cfg(unix)]
    {
        crate::rclone_copy::socket_for(point)
            .filter(|socket| socket.exists())
            .map(At::Socket)
    }
    #[cfg(not(unix))]
    {
        crate::winfsp::rc_of(point)
    }
}

/// How often what waits is asked again while it is sent.
const POLL: Duration = Duration::from_millis(200);

/// How long rclone may take to say what waits: it answers from memory, so
/// a longer wait is a mount held by a server that stopped answering.
const ANSWERED_WITHIN: Duration = Duration::from_secs(5);

/// A call to the remote control at `at`, given up after
/// `ANSWERED_WITHIN`.
fn asked(
    at: &At,
    method: &str,
    args: &serde_json::Value,
    cancel: &CancelToken,
) -> anyhow::Result<serde_json::Value> {
    let within = cancel.child();
    let answer = std::thread::scope(|scope| {
        let (done, finished) = crossbeam_channel::bounded::<()>(0);
        let given_up = within.clone();
        scope.spawn(move || {
            if finished.recv_timeout(ANSWERED_WITHIN)
                == Err(crossbeam_channel::RecvTimeoutError::Timeout)
            {
                given_up.cancel();
            }
        });
        // Dropped once answered, which ends the wait above at once.
        let _done = done;
        call(at, method, args, &within)
    });
    if within.is_cancelled() && !cancel.is_cancelled() {
        anyhow::bail!(
            "rclone did not answer within {} s: its server may have stopped answering",
            ANSWERED_WITHIN.as_secs()
        );
    }
    answer
}

/// Tries after which an upload is the server refusing it: rclone goes on
/// trying, a copy waiting for it says so.
const REFUSED: u64 = 3;

/// The cache the mount whose remote control is at `at` keeps
/// (`options/get`'s `vfs.CacheMode`): `off`, `minimal`, `writes`, `full`.
///
/// # Errors
/// When its remote control does not answer, or the wait was cancelled.
pub fn cache_mode(at: &At, cancel: &CancelToken) -> anyhow::Result<String> {
    let options = asked(at, "options/get", &serde_json::json!({}), cancel)?;
    options["vfs"]["CacheMode"]
        .as_str()
        .map(str::to_owned)
        .ok_or_else(|| anyhow::anyhow!("rclone did not say which cache it keeps"))
}

/// Whether a mount keeping `mode` sends what is written after its `close`.
pub fn sends_later(mode: &str) -> bool {
    matches!(mode, "writes" | "full")
}

/// An upload waiting in the mount's queue.
#[derive(Clone, Debug, PartialEq)]
struct Waiting {
    id: u64,
    name: String,
    tries: u64,
    uploading: bool,
}

/// What waits to be sent, as the mount's queue says (`vfs/queue`).
fn queue(at: &At, cancel: &CancelToken) -> anyhow::Result<Vec<Waiting>> {
    let answer = asked(at, "vfs/queue", &serde_json::json!({}), cancel)?;
    Ok(waiting_in(&answer))
}

/// The uploads a `vfs/queue` answer lists.
fn waiting_in(answer: &serde_json::Value) -> Vec<Waiting> {
    answer["queue"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|item| {
            Some(Waiting {
                id: item["id"].as_u64()?,
                name: item["name"].as_str().unwrap_or_default().to_owned(),
                tries: item["tries"].as_u64().unwrap_or(0),
                uploading: item["uploading"].as_bool().unwrap_or(false),
            })
        })
        .collect()
}

/// How many files the mount still sends: queued and going, as its queue
/// says (`vfs/queue`, answered from memory where `vfs/stats` may wait on
/// a read the server never answers).
///
/// # Errors
/// When its remote control does not answer, or the wait was cancelled.
pub fn waiting(at: &At, cancel: &CancelToken) -> anyhow::Result<usize> {
    Ok(queue(at, cancel)?.len())
}

/// Have the server take now what waits to be sent, and return once it
/// has: what waited when asked, not what is written meanwhile. `left` is
/// told how many of those remain; `pause` is called between looks, to hold
/// while a plan is paused.
///
/// # Errors
/// When an upload was tried `REFUSED` times (it stays in rclone's cache,
/// which goes on trying), the remote control stops answering, or the wait
/// was cancelled.
pub fn send_now(
    at: &At,
    cancel: &CancelToken,
    pause: &dyn Fn(),
    left: &mut dyn FnMut(usize),
) -> anyhow::Result<()> {
    let ours: HashSet<u64> = queue(at, cancel)?.iter().map(|w| w.id).collect();
    let cancelled = cancel.subscribe();
    loop {
        pause();
        if cancel.is_cancelled() {
            return Err(Cancelled.into());
        }
        let waiting: Vec<Waiting> = queue(at, cancel)?
            .into_iter()
            .filter(|w| ours.contains(&w.id))
            .collect();
        left(waiting.len());
        if waiting.is_empty() {
            return Ok(());
        }
        if let Some(refused) = waiting.iter().find(|w| w.tries >= REFUSED && !w.uploading) {
            anyhow::bail!(
                "the server refused “{}” {} times: rclone keeps it on this disk and tries again",
                refused.name,
                refused.tries
            );
        }
        // Each waits out rclone's delay (`--vfs-write-back`), and its delay
        // before trying again: asked to go now.
        for w in waiting.iter().filter(|w| !w.uploading) {
            let now = serde_json::json!({ "id": w.id, "expiry": 0, "relative": false });
            if let Err(error) = asked(at, "vfs/queue-set-expiry", &now, cancel) {
                // Gone meanwhile: sent, which the next look sees.
                tracing::debug!("rclone did not hurry “{}”: {error:#}", w.name);
            }
        }
        crossbeam_channel::select! {
            recv(cancelled.receiver()) -> _ => {}
            default(POLL) => {}
        }
    }
}

/// How long a file written may take to reach rclone's queue after it was
/// closed (the kernel's release of it, which rclone queues it at): longer,
/// a program still holds it open there, and the write-out fails (a move's
/// original stays) rather than end before the server has it.
const RELEASED_WITHIN: Duration = Duration::from_secs(60);

/// The most rclone's record of one file in its cache takes: what it holds
/// and was read of it, a few ranges.
const RECORD_MOST: u64 = 1 << 20;

/// How a file stands in a mount's cache, by rclone's record of it.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Record {
    /// Written and not on the server yet (`"Dirty": true`), whether or not
    /// rclone has queued it.
    Unsent,
    /// On the server: its record says so, or there is none (rclone lets go
    /// of nothing it has not sent).
    Sent,
    /// Not as rclone writes its records.
    Unknown,
}

/// rclone's record of the file `name` (its path on the mount) in the
/// folder of records `records` (`vfs/stats`' `pathMeta`).
fn record_of(records: &Path, name: &Path) -> Record {
    match skwir_core::fs::file::read_regular(&records.join(name), RECORD_MOST) {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Record::Sent,
        Err(_) => Record::Unknown,
        Ok(bytes) => match serde_json::from_slice::<serde_json::Value>(&bytes)
            .ok()
            .and_then(|record| record["Dirty"].as_bool())
        {
            Some(true) => Record::Unsent,
            Some(false) => Record::Sent,
            None => Record::Unknown,
        },
    }
}

/// The name rclone gives in its queue to the file at `name` on its mount.
fn queued_as(name: &Path) -> String {
    name.components()
        .map(|part| part.as_os_str().to_string_lossy())
        .collect::<Vec<_>>()
        .join("/")
}

/// Of `written` (copies made on the mount at `point`: files, folders with
/// all they hold), the files the mount holds unsent, by their name in its
/// queue: read from rclone's records (`records`), on this disk. `None`
/// where a record is not as rclone writes them.
fn unsent_of(
    records: &Path,
    point: &Path,
    written: &[PathBuf],
) -> Option<BTreeMap<String, PathBuf>> {
    let mut unsent = BTreeMap::new();
    let mut looked: Vec<PathBuf> = written
        .iter()
        .filter_map(|path| path.strip_prefix(point).ok())
        .map(Path::to_path_buf)
        .collect();
    while let Some(name) = looked.pop() {
        let at = records.join(&name);
        match std::fs::symlink_metadata(&at) {
            Ok(meta) if meta.is_dir() => {
                let held = std::fs::read_dir(&at).ok()?;
                looked.extend(held.flatten().map(|entry| name.join(entry.file_name())));
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            _ => match record_of(records, &name) {
                Record::Unsent => {
                    unsent.insert(queued_as(&name), name);
                }
                Record::Sent => {}
                Record::Unknown => return None,
            },
        }
    }
    Some(unsent)
}

/// Have the server take now what was `written` on the mount at `point`
/// (each copy a plan made there: a file, or a folder and all it holds)
/// whose remote control is at `at`, and return once it has: a file not
/// queued yet (its release has not reached rclone) is waited for, then
/// sent now with the others. `left` is told how many files the server
/// still takes; `pause` is called between looks, to hold while a plan is
/// paused. What waits of other programs' is not waited for. Where the
/// mount's records cannot be read as rclone writes them, what waits when
/// asked is sent ([`send_now`]).
///
/// # Errors
/// When an upload was tried `REFUSED` times (it stays in rclone's cache,
/// which goes on trying), a file written never reached the queue within
/// `RELEASED_WITHIN`, the remote control stops answering, or the wait was
/// cancelled.
pub fn send_written(
    at: &At,
    point: &Path,
    written: &[PathBuf],
    cancel: &CancelToken,
    pause: &dyn Fn(),
    left: &mut dyn FnMut(usize),
) -> anyhow::Result<()> {
    let stats = asked(at, "vfs/stats", &serde_json::json!({}), cancel)?;
    let records = stats["diskCache"]["pathMeta"].as_str().map(PathBuf::from);
    let Some((records, mut unsent)) = records.and_then(|records| {
        let unsent = unsent_of(&records, point, written)?;
        Some((records, unsent))
    }) else {
        tracing::debug!("rclone's records of its cache are not read: what waits is sent");
        return send_now(at, cancel, pause, left);
    };
    let began = std::time::Instant::now();
    // Seen in the queue once: its release came, and it is sent once its
    // record says so, however long the server takes it.
    let mut queued: HashSet<String> = HashSet::new();
    let cancelled = cancel.subscribe();
    loop {
        pause();
        if cancel.is_cancelled() {
            return Err(Cancelled.into());
        }
        let waiting: HashMap<String, Waiting> = queue(at, cancel)?
            .into_iter()
            .map(|w| (w.name.clone(), w))
            .collect();
        queued.extend(
            unsent
                .keys()
                .filter(|name| waiting.contains_key(*name))
                .cloned(),
        );
        // Out of the queue, a file is sent once its record says so: before
        // its release, it is unsent and not queued yet.
        unsent.retain(|name, file| {
            waiting.contains_key(name) || record_of(&records, file) != Record::Sent
        });
        left(unsent.len());
        if unsent.is_empty() {
            return Ok(());
        }
        let ours = || unsent.keys().filter_map(|name| waiting.get(name));
        if let Some(refused) = ours().find(|w| w.tries >= REFUSED && !w.uploading) {
            anyhow::bail!(
                "the server refused “{}” {} times: rclone keeps it on this disk and tries again",
                refused.name,
                refused.tries
            );
        }
        if began.elapsed() >= RELEASED_WITHIN
            && let Some(name) = unsent.keys().find(|name| !queued.contains(*name))
        {
            anyhow::bail!(
                "“{name}” never reached rclone's queue: a program still holds it open there, and the server does not have it"
            );
        }
        // Each waits out rclone's delay (`--vfs-write-back`), and its delay
        // before trying again: asked to go now.
        for w in ours().filter(|w| !w.uploading) {
            let now = serde_json::json!({ "id": w.id, "expiry": 0, "relative": false });
            if let Err(error) = asked(at, "vfs/queue-set-expiry", &now, cancel) {
                // Gone meanwhile: sent, which the next look sees.
                tracing::debug!("rclone did not hurry “{}”: {error:#}", w.name);
            }
        }
        crossbeam_channel::select! {
            recv(cancelled.receiver()) -> _ => {}
            default(POLL) => {}
        }
    }
}

/// What a mount Skwir made sends to its server after a file is closed,
/// written out before a plan ends, as onto a drive the user can unplug
/// (ADR-102): a transfer that copies nothing, on every system.
#[derive(Default)]
pub struct RcloneSends {
    /// Whether each mount sends what is written after its `close`, by its
    /// point and the system's number for that mount: one made again there
    /// may keep another cache.
    sends: Mutex<HashMap<(PathBuf, u64), bool>>,
}

impl Transfer for RcloneSends {
    fn name(&self) -> &'static str {
        "rclone-sends"
    }

    /// It copies nothing: Skwir's own copy, or rclone's within a mount
    /// (`RcloneCopy`), writes there.
    fn serves(&self, _from: &Path, _into: &Path) -> bool {
        false
    }

    fn copy(
        &self,
        _from: &Path,
        _names: &[OsString],
        _into: &Path,
        _aside: &Aside<'_>,
        _told: &Told<'_>,
        _control: &Control,
    ) -> anyhow::Result<()> {
        Err(NotServed.into())
    }

    /// A mount Skwir made whose cache keeps what is written and sends it
    /// after its `close` (`writes`, `full`): asked of its remote control
    /// once a mount (`options/get`).
    fn sends_later(&self, folder: &Path) -> bool {
        let Some(remote) = skwir_core::fs::reach::of(folder) else {
            return false;
        };
        if remote.fstype != crate::rclone::FSTYPE {
            return false;
        }
        let mount = (
            remote.point.clone(),
            remote.device.map_or(0, skwir_core::fs::reach::mount_of),
        );
        if let Some(sends) = self.sends.lock().get(&mount) {
            return *sends;
        }
        let Some(at) = control_of(&remote.point) else {
            return false;
        };
        let sends = match cache_mode(&at, &CancelToken::new()) {
            Ok(mode) => sends_later(&mode),
            // Not known: a later copy asks again.
            Err(error) => {
                tracing::debug!("rclone's remote control did not say its cache: {error:#}");
                return false;
            }
        };
        self.sends.lock().insert(mount, sends);
        sends
    }

    /// What the plan wrote on the mount is sent now, and waited for
    /// ([`send_written`]).
    fn send(
        &self,
        folder: &Path,
        written: &[PathBuf],
        control: &Control,
        left: &mut dyn FnMut(usize),
    ) -> anyhow::Result<()> {
        let Some((point, at)) = skwir_core::fs::reach::of(folder).and_then(|remote| {
            let at = control_of(&remote.point)?;
            Some((remote.point.clone(), at))
        }) else {
            anyhow::bail!(
                "rclone's mount of “{}” is gone before its server had what was written",
                folder.display()
            );
        };
        send_written(
            &at,
            &point,
            written,
            control.cancel(),
            &|| control.wait_while_paused(),
            left,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_queue_rclone_answers_is_read_as_it_says() {
        let answer = serde_json::json!({
            "queue": [
                { "delay": 5, "expiry": 4.9, "id": 1, "name": "l'été/notes.txt",
                  "size": 2, "tries": 0, "uploading": false },
                { "id": 2, "name": "a b.bin", "tries": 3, "uploading": true },
                { "name": "no id" },
            ]
        });
        assert_eq!(
            waiting_in(&answer),
            [
                Waiting {
                    id: 1,
                    name: "l'été/notes.txt".into(),
                    tries: 0,
                    uploading: false,
                },
                Waiting {
                    id: 2,
                    name: "a b.bin".into(),
                    tries: 3,
                    uploading: true,
                },
            ]
        );
        assert!(waiting_in(&serde_json::json!({})).is_empty());
        assert!(sends_later("full") && sends_later("writes"));
        assert!(!sends_later("minimal") && !sends_later("off"));
    }

    /// What a plan wrote is unsent while rclone's record of it says so,
    /// queued or not (its release not come yet): a file, or each file of
    /// a folder, by the name the queue gives it. None recorded is sent; a
    /// record rclone would not write leaves the plan to what waits.
    #[test]
    fn what_was_written_is_unsent_while_rclone_s_record_says_so() {
        let tmp = tempfile::tempdir().unwrap();
        let records = tmp.path().join("vfsMeta").join(":webdav{Ab12}");
        let point = Path::new("/example/mounts/rclone dav:%2F%2F127.0.0.1:8080%2F");
        let record = |name: &str, dirty: Option<bool>| {
            let at = records.join(name);
            std::fs::create_dir_all(at.parent().unwrap()).unwrap();
            let text = match dirty {
                Some(dirty) => {
                    format!(r#"{{"Size": 5, "Rs": null, "Fingerprint": "", "Dirty": {dirty}}}"#)
                }
                None => r#"{"Size": 5}"#.to_owned(),
            };
            std::fs::write(at, text).unwrap();
        };
        record("l'été — a note.txt", Some(true));
        record("sent.txt", Some(false));
        record("a folder/one.txt", Some(true));
        record("a folder/inner \"q\"/two.txt", Some(true));
        record("a folder/inner \"q\"/sent.txt", Some(false));
        let written: Vec<PathBuf> = [
            "l'été — a note.txt",
            "sent.txt",
            "a folder",
            "never kept.txt",
        ]
        .iter()
        .map(|name| point.join(name))
        .chain([PathBuf::from("/elsewhere/not on it.txt")])
        .collect();
        let unsent = unsent_of(&records, point, &written).unwrap();
        assert_eq!(
            unsent.keys().map(String::as_str).collect::<Vec<_>>(),
            [
                "a folder/inner \"q\"/two.txt",
                "a folder/one.txt",
                "l'été — a note.txt"
            ]
        );
        assert_eq!(
            unsent["a folder/one.txt"],
            Path::new("a folder").join("one.txt")
        );
        assert_eq!(record_of(&records, Path::new("sent.txt")), Record::Sent);
        assert_eq!(record_of(&records, Path::new("gone.txt")), Record::Sent);
        record("other.txt", None);
        assert_eq!(record_of(&records, Path::new("other.txt")), Record::Unknown);
        assert!(unsent_of(&records, point, &[point.join("other.txt")]).is_none());
    }
}
