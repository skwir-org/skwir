//! What a mount Skwir made sends to its server after a file is closed
//! (ADR-102): with rclone's `writes` or `full` cache, a file written is
//! kept on this disk and sent a few seconds after its `close`, and tried
//! again while the server refuses it. Asked of the mount's remote control
//! (`vfs/queue`, `vfs/stats`), sent now (`vfs/queue-set-expiry`) and waited
//! for, so a copy is done when the server has it (`RcloneCopy::send`) and
//! a Disconnect loses nothing.

use std::collections::HashSet;
use std::path::Path;
use std::time::Duration;

use skwir_core::CancelToken;
use skwir_core::op::Cancelled;

use crate::rclone_copy::call;

/// How often what waits is asked again while it is sent.
const POLL: Duration = Duration::from_millis(200);

/// How long rclone may take to say what waits: it answers from memory, so
/// a longer wait is a mount held by a server that stopped answering.
const ANSWERED_WITHIN: Duration = Duration::from_secs(5);

/// A call to the remote control on `socket`, given up after
/// `ANSWERED_WITHIN`.
fn asked(
    socket: &Path,
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
        call(socket, method, args, &within)
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

/// The cache the mount whose remote control is `socket` keeps
/// (`options/get`'s `vfs.CacheMode`): `off`, `minimal`, `writes`, `full`.
///
/// # Errors
/// When its remote control does not answer, or the wait was cancelled.
pub fn cache_mode(socket: &Path, cancel: &CancelToken) -> anyhow::Result<String> {
    let options = asked(socket, "options/get", &serde_json::json!({}), cancel)?;
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
fn queue(socket: &Path, cancel: &CancelToken) -> anyhow::Result<Vec<Waiting>> {
    let answer = asked(socket, "vfs/queue", &serde_json::json!({}), cancel)?;
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
pub fn waiting(socket: &Path, cancel: &CancelToken) -> anyhow::Result<usize> {
    Ok(queue(socket, cancel)?.len())
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
    socket: &Path,
    cancel: &CancelToken,
    pause: &dyn Fn(),
    left: &mut dyn FnMut(usize),
) -> anyhow::Result<()> {
    let ours: HashSet<u64> = queue(socket, cancel)?.iter().map(|w| w.id).collect();
    let cancelled = cancel.subscribe();
    loop {
        pause();
        if cancel.is_cancelled() {
            return Err(Cancelled.into());
        }
        let waiting: Vec<Waiting> = queue(socket, cancel)?
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
            if let Err(error) = asked(socket, "vfs/queue-set-expiry", &now, cancel) {
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
}
