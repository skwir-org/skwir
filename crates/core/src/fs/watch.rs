//! Directory watching, coalesced into named changes.
//!
//! On Linux this talks to inotify directly: adding a non-recursive watch is
//! two syscalls with no thread round trip, which keeps `open` in the tens of
//! microseconds. Elsewhere the `notify` crate is used.

use std::collections::HashSet;
use std::ffi::OsString;
use std::path::Path;
use std::time::Duration;

use crossbeam_channel::Receiver;

use crate::cancel::CancelToken;

/// A coalesced change: the names in the watched directory that were touched.
pub struct Changes {
    pub names: HashSet<OsString>,
    /// Something changed that we could not attribute to a name: relist.
    pub rescan: bool,
}

/// Keeps the watch alive; dropping it stops watching.
pub struct WatchHandle {
    _inner: Box<dyn std::any::Any + Send>,
}

/// Watch `dir` (non-recursive). Changes are coalesced over `window` and
/// delivered on the returned receiver until `cancel` fires or the handle drops.
pub fn watch(
    dir: &Path,
    window: Duration,
    cancel: &CancelToken,
) -> std::io::Result<(WatchHandle, Receiver<Changes>)> {
    imp::watch(dir, window, cancel)
}

#[cfg(target_os = "linux")]
mod imp {
    use std::collections::HashSet;
    use std::path::Path;
    use std::time::{Duration, Instant};

    use crossbeam_channel::{Receiver, Sender, unbounded};
    use inotify::{EventMask, Inotify, WatchMask};
    use rustix::event::{PollFd, PollFlags};

    use super::{Changes, WatchHandle};
    use crate::cancel::CancelToken;

    pub fn watch(
        dir: &Path,
        window: Duration,
        cancel: &CancelToken,
    ) -> std::io::Result<(WatchHandle, Receiver<Changes>)> {
        let mut inotify = Inotify::init()?;
        let mask = WatchMask::CREATE
            | WatchMask::DELETE
            | WatchMask::MODIFY
            | WatchMask::ATTRIB
            | WatchMask::CLOSE_WRITE
            | WatchMask::MOVED_FROM
            | WatchMask::MOVED_TO
            | WatchMask::DELETE_SELF
            | WatchMask::MOVE_SELF;
        inotify.watches().add(dir, mask)?;
        let (tx, rx) = unbounded();
        let stop = cancel.child();
        let thread_stop = stop.clone();
        std::thread::Builder::new()
            .name("bibli-inotify".into())
            .spawn(move || run(&mut inotify, &tx, window, &thread_stop))?;
        Ok((
            WatchHandle {
                _inner: Box::new(StopOnDrop(stop)),
            },
            rx,
        ))
    }

    struct StopOnDrop(CancelToken);
    impl Drop for StopOnDrop {
        fn drop(&mut self) {
            self.0.cancel();
        }
    }

    /// Wait up to `timeout` for the inotify fd to become readable.
    fn wait_readable(inotify: &Inotify, timeout: Duration) -> bool {
        let mut fds = [PollFd::new(inotify, PollFlags::IN)];
        let ts = rustix::time::Timespec {
            tv_sec: i64::try_from(timeout.as_secs()).unwrap_or(i64::MAX),
            tv_nsec: i64::from(timeout.subsec_nanos()),
        };
        matches!(rustix::event::poll(&mut fds, Some(&ts)), Ok(n) if n > 0)
    }

    fn run(inotify: &mut Inotify, out: &Sender<Changes>, window: Duration, stop: &CancelToken) {
        let mut buf = vec![0u8; 64 * 1024];
        loop {
            if stop.is_cancelled() {
                return;
            }
            if !wait_readable(inotify, Duration::from_millis(250)) {
                continue;
            }
            let mut changes = Changes {
                names: HashSet::new(),
                rescan: false,
            };
            let deadline = Instant::now() + window;
            loop {
                match inotify.read_events(&mut buf) {
                    Ok(events) => {
                        for ev in events {
                            if ev.mask.intersects(
                                EventMask::Q_OVERFLOW
                                    | EventMask::DELETE_SELF
                                    | EventMask::MOVE_SELF
                                    | EventMask::IGNORED,
                            ) {
                                changes.rescan = true;
                            } else if let Some(name) = ev.name {
                                changes.names.insert(name.to_os_string());
                            }
                        }
                    }
                    Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {}
                    Err(_) => {
                        changes.rescan = true;
                        break;
                    }
                }
                let left = deadline.saturating_duration_since(Instant::now());
                if left.is_zero() || !wait_readable(inotify, left) {
                    break;
                }
            }
            if stop.is_cancelled() || out.send(changes).is_err() {
                return;
            }
        }
    }
}

#[cfg(not(target_os = "linux"))]
mod imp {
    use std::collections::HashSet;
    use std::path::{Path, PathBuf};
    use std::sync::Arc;
    use std::time::Duration;

    use crossbeam_channel::{Receiver, Sender, unbounded};
    use notify::{EventKind, RecursiveMode, Watcher};

    use super::{Changes, WatchHandle};
    use crate::cancel::CancelToken;

    pub fn watch(
        dir: &Path,
        window: Duration,
        cancel: &CancelToken,
    ) -> std::io::Result<(WatchHandle, Receiver<Changes>)> {
        let cancel = cancel.clone();
        let (raw_tx, raw_rx) = unbounded::<notify::Result<notify::Event>>();
        let mut watcher = notify::recommended_watcher(move |res| {
            let _ = raw_tx.send(res);
        })
        .map_err(std::io::Error::other)?;
        watcher
            .watch(dir, RecursiveMode::NonRecursive)
            .map_err(std::io::Error::other)?;
        let (tx, rx) = unbounded();
        let dir: Arc<PathBuf> = Arc::new(dir.to_path_buf());
        std::thread::Builder::new()
            .name("bibli-watch".into())
            .spawn(move || coalesce(&raw_rx, &tx, &dir, window, &cancel))?;
        Ok((
            WatchHandle {
                _inner: Box::new(watcher),
            },
            rx,
        ))
    }

    fn coalesce(
        raw: &Receiver<notify::Result<notify::Event>>,
        out: &Sender<Changes>,
        dir: &Path,
        window: Duration,
        cancel: &CancelToken,
    ) {
        loop {
            let first = match raw.recv_timeout(Duration::from_millis(250)) {
                Ok(ev) => ev,
                Err(crossbeam_channel::RecvTimeoutError::Timeout) => {
                    if cancel.is_cancelled() {
                        return;
                    }
                    continue;
                }
                Err(crossbeam_channel::RecvTimeoutError::Disconnected) => return,
            };
            let mut changes = Changes {
                names: HashSet::new(),
                rescan: false,
            };
            absorb(&mut changes, first, dir);
            let deadline = std::time::Instant::now() + window;
            while let Ok(ev) = raw.recv_deadline(deadline) {
                absorb(&mut changes, ev, dir);
            }
            if cancel.is_cancelled() || out.send(changes).is_err() {
                return;
            }
        }
    }

    fn absorb(changes: &mut Changes, ev: notify::Result<notify::Event>, dir: &Path) {
        let Ok(ev) = ev else {
            changes.rescan = true;
            return;
        };
        if ev.need_rescan() {
            changes.rescan = true;
            return;
        }
        if matches!(ev.kind, EventKind::Access(_)) {
            return;
        }
        for p in ev.paths {
            if p.parent() == Some(dir)
                && let Some(name) = p.file_name()
            {
                changes.names.insert(name.to_os_string());
            } else if p == dir {
                changes.rescan = true;
            }
        }
    }
}
