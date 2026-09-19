//! Optional playback process. All process and pixel I/O lives on its manager;
//! UI methods only enqueue controls or take the latest bounded update.

use crate::process::ChildGuard;
use anyhow::Context;
use bibli_core::{
    CancelToken,
    location::Waker,
    media::{Control, Frame, MAX_MESSAGE, MediaSession, PIXELS_FILE, Reply, Request, Update},
};
use parking_lot::Mutex;
use std::{
    fs::OpenOptions,
    io::{BufRead, BufReader, Read, Seek, SeekFrom, Write},
    path::PathBuf,
    process::{Command, Stdio},
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
        mpsc,
    },
    time::{Duration, Instant},
};

pub fn worker_path() -> Option<PathBuf> {
    std::env::var_os("BIBLI_MEDIA_WORKER")
        .map(PathBuf::from)
        .or_else(|| {
            let path = std::env::current_exe()
                .ok()?
                .parent()?
                .join("bibli-media-worker");
            path.is_file().then_some(path)
        })
        .or_else(|| crate::process::find("bibli-media-worker"))
}

struct Shared {
    update: Mutex<Update>,
    generation: AtomicU64,
    wake: Waker,
}
impl Shared {
    fn state(&self, generation: u64, state: bibli_core::media::Playback) {
        let mut update = self.update.lock();
        if generation != self.generation.load(Ordering::Acquire) {
            return;
        }
        let empty = update.state.is_none() && update.frame.is_none() && update.error.is_none();
        update.state = Some(state);
        drop(update);
        if empty {
            (self.wake)();
        }
    }
    fn frame(&self, generation: u64, frame: Frame) {
        let mut update = self.update.lock();
        if generation != self.generation.load(Ordering::Acquire) {
            return;
        }
        let empty = update.state.is_none() && update.frame.is_none() && update.error.is_none();
        let previous = update.frame.replace(frame);
        drop(update);
        drop(previous);
        if empty {
            (self.wake)();
        }
    }
    fn publish(&self, apply: impl FnOnce(&mut Update)) {
        let mut update = self.update.lock();
        let empty = update.state.is_none() && update.frame.is_none() && update.error.is_none();
        apply(&mut update);
        drop(update);
        if empty {
            (self.wake)();
        }
    }
}
pub struct WorkerSession {
    commands: crossbeam_channel::Sender<Request>,
    shared: Arc<Shared>,
    lifetime: CancelToken,
}
impl Drop for WorkerSession {
    fn drop(&mut self) {
        self.lifetime.cancel();
    }
}
impl MediaSession for WorkerSession {
    fn control(&self, command: Control) -> anyhow::Result<()> {
        anyhow::ensure!(command.valid(), "invalid playback control");
        anyhow::ensure!(!self.lifetime.is_cancelled(), "video session closed");
        // Serialize publication with frame acceptance. A failed enqueue must not
        // invalidate the worker's generation and permanently hide its frames.
        let mut update = self.shared.update.lock();
        let generation = self.shared.generation.load(Ordering::Relaxed) + 1;
        self.commands
            .try_send(Request::Control {
                generation,
                command,
            })
            .context("video controls busy or session closed")?;
        self.shared.generation.store(generation, Ordering::Release);
        update.frame = None;
        update.state = None;
        Ok(())
    }
    fn take_update(&self) -> Update {
        std::mem::take(&mut *self.shared.update.lock())
    }
}
impl WorkerSession {
    pub fn open(
        path: PathBuf,
        lifetime: &CancelToken,
        wake: Waker,
    ) -> anyhow::Result<Option<Arc<dyn MediaSession>>> {
        let Some(executable) = worker_path() else {
            return Ok(None);
        };
        Self::open_with_worker(executable, path, lifetime, wake)
    }

    fn open_with_worker(
        executable: PathBuf,
        path: PathBuf,
        lifetime: &CancelToken,
        wake: Waker,
    ) -> anyhow::Result<Option<Arc<dyn MediaSession>>> {
        let lifetime = lifetime.child();
        let shared = Arc::new(Shared {
            update: Mutex::new(Update::default()),
            generation: AtomicU64::new(0),
            wake,
        });
        let (commands, rx) = crossbeam_channel::bounded(32);
        let (startup, ready) = mpsc::sync_channel(1);
        let manager_shared = shared.clone();
        let manager_lifetime = lifetime.clone();
        std::thread::Builder::new()
            .name("bibli-video-session".into())
            .spawn(move || {
                let result = manage(
                    executable,
                    path,
                    &manager_lifetime,
                    &manager_shared,
                    &rx,
                    &startup,
                );
                if let Err(error) = result {
                    let message = error.to_string();
                    let _ = startup.try_send(Err(anyhow::anyhow!("{message}")));
                    if !manager_lifetime.is_cancelled() {
                        manager_shared.publish(|u| u.error = Some(message));
                    }
                }
                manager_lifetime.cancel();
            })?;
        let start = Instant::now();
        loop {
            match ready.recv_timeout(Duration::from_millis(10)) {
                Ok(Ok(true)) => {
                    return Ok(Some(Arc::new(Self {
                        commands,
                        shared,
                        lifetime,
                    })));
                }
                Ok(Ok(false)) => return Ok(None),
                Ok(Err(error)) => return Err(error),
                Err(mpsc::RecvTimeoutError::Timeout) => {
                    if lifetime.is_cancelled() || start.elapsed() > Duration::from_secs(20) {
                        lifetime.cancel();
                        anyhow::bail!("video open cancelled or timed out");
                    }
                }
                Err(mpsc::RecvTimeoutError::Disconnected) => {
                    anyhow::bail!("video worker stopped during startup")
                }
            }
        }
    }
}

fn manage(
    executable: PathBuf,
    path: PathBuf,
    lifetime: &CancelToken,
    shared: &Shared,
    commands: &crossbeam_channel::Receiver<Request>,
    startup: &mpsc::SyncSender<anyhow::Result<bool>>,
) -> anyhow::Result<()> {
    // One private reusable slot, RAM-backed on Linux. Never one file per frame.
    #[cfg(target_os = "linux")]
    let scratch = tempfile::Builder::new()
        .prefix("bibli-video-")
        .tempdir_in("/dev/shm")?;
    #[cfg(not(target_os = "linux"))]
    let scratch = tempfile::tempdir()?;
    let mut pixels = OpenOptions::new()
        .read(true)
        .write(true)
        .create_new(true)
        .open(scratch.path().join(PIXELS_FILE))?;
    pixels.set_len(u64::try_from(bibli_core::media::frame_bytes(
        bibli_core::media::MAX_SIDE,
        bibli_core::media::MAX_SIDE,
    )?)?)?;
    let mut command = Command::new(executable);
    command
        .arg(path)
        .arg(scratch.path())
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null());
    // Decoder/overlay threads are recreated during track changes. glibc's
    // CPU-count-based arena limit otherwise retains hundreds of MB of freed
    // frame buffers across those threads. Bound fragmentation in this child.
    #[cfg(target_env = "gnu")]
    command.env("MALLOC_ARENA_MAX", "2");
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        command.process_group(0);
    }
    let mut child = ChildGuard(command.spawn().context("start bibli-media-worker")?);
    let mut input = child.0.stdin.take().context("video worker stdin")?;
    #[cfg(unix)]
    {
        // A wedged decoder must not trap its manager in a full command pipe;
        // cancellation still needs to reach ChildGuard and reap the process.
        let flags = rustix::fs::fcntl_getfl(&input)?;
        rustix::fs::fcntl_setfl(&input, flags | rustix::fs::OFlags::NONBLOCK)?;
    }
    let output = child.0.stdout.take().context("video worker stdout")?;
    let (tx, replies) = crossbeam_channel::bounded(2);
    std::thread::Builder::new()
        .name("bibli-video-replies".into())
        .spawn(move || {
            let mut reader = BufReader::new(output);
            loop {
                let mut bytes = Vec::new();
                let result = (|| -> anyhow::Result<Reply> {
                    let n = Read::by_ref(&mut reader)
                        .take((MAX_MESSAGE + 1) as u64)
                        .read_until(b'\n', &mut bytes)?;
                    anyhow::ensure!(
                        n > 0 && n <= MAX_MESSAGE && bytes.last() == Some(&b'\n'),
                        "invalid or closed video worker output"
                    );
                    Ok(serde_json::from_slice(&bytes)?)
                })();
                let failed = result.is_err();
                if tx.send(result).is_err() || failed {
                    break;
                }
            }
        })?;
    let send = |input: &mut std::process::ChildStdin, request: &Request| -> anyhow::Result<()> {
        serde_json::to_writer(&mut *input, request)?;
        input.write_all(b"\n")?;
        input.flush()?;
        Ok(())
    };
    let started = Instant::now();
    let mut ready = false;
    while !lifetime.is_cancelled() {
        anyhow::ensure!(
            ready || started.elapsed() < Duration::from_secs(18),
            "video worker timed out"
        );
        let reply = crossbeam_channel::select! {
            recv(commands) -> request => {
                let Ok(request) = request else { break; };
                send(&mut input, &request)?;
                continue;
            }
            recv(replies) -> reply => {
                let result = reply.context("video worker exited")?;
                if result.is_err() && !ready && child.0.try_wait()?.is_some_and(|status| status.code() == Some(127)) {
                    // A dynamic loader reports missing shared libraries with 127.
                    // Decoder/file errors have a protocol reply and never fall back.
                    let _ = startup.send(Ok(false));
                    return Ok(());
                }
                result?
            },
            default(Duration::from_millis(100)) => continue,
        };
        match reply {
            Reply::Ready => {
                anyhow::ensure!(!ready, "duplicate video greeting");
                ready = true;
                let _ = startup.send(Ok(true));
            }
            Reply::Unavailable(_) => {
                let _ = startup.send(Ok(false));
                return Ok(());
            }
            Reply::Error(error) => anyhow::bail!("{error}"),
            Reply::State {
                generation,
                playback,
            } => shared.state(generation, playback),
            Reply::Frame {
                generation,
                width,
                height,
                ..
            } => {
                let mut bgra = vec![0; bibli_core::media::frame_bytes(width, height)?];
                pixels.seek(SeekFrom::Start(0))?;
                pixels.read_exact(&mut bgra)?;
                shared.frame(
                    generation,
                    Frame {
                        width,
                        height,
                        bgra,
                    },
                );
                send(&mut input, &Request::Consumed)?;
            }
        }
    }
    // ChildGuard reaps before scratch is removed. Neither runs on the UI.
    Ok(())
}

#[cfg(all(test, target_os = "linux"))]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;

    fn until(mut condition: impl FnMut() -> bool) {
        let deadline = Instant::now() + Duration::from_secs(3);
        while !condition() {
            assert!(Instant::now() < deadline, "media condition timed out");
            std::thread::sleep(Duration::from_millis(5));
        }
    }
    fn fixture(script: &str) -> (tempfile::TempDir, PathBuf, PathBuf) {
        let root = tempfile::tempdir().unwrap();
        let source = root.path().join("Synthetic clip's sample.mp4");
        std::fs::write(&source, b"synthetic").unwrap();
        let worker = root.path().join("worker");
        let shell = crate::process::required("sh").unwrap();
        std::fs::write(&worker, format!("#!{}\nprintf '%s' \"$$\" > \"$1.pid\"\nprintf '%s' \"$2\" > \"$1.scratch\"\n{script}", shell.display())).unwrap();
        std::fs::set_permissions(&worker, std::fs::Permissions::from_mode(0o700)).unwrap();
        (root, source, worker)
    }
    #[test]
    fn closing_and_disabling_reap_idle_player_and_frame_slot() {
        for disable in [false, true] {
            let (_root, source, worker) =
                fixture("printf '%s\\n' '\"Ready\"'\nwhile IFS= read -r line; do :; done\n");
            let lifetime = CancelToken::new();
            let mut session =
                WorkerSession::open_with_worker(worker, source.clone(), &lifetime, Arc::new(|| {}))
                    .unwrap();
            let pid = std::fs::read_to_string(format!("{}.pid", source.display())).unwrap();
            let scratch = std::fs::read_to_string(format!("{}.scratch", source.display())).unwrap();
            if disable {
                lifetime.cancel();
            } else {
                drop(session.take());
            }
            until(|| {
                !std::path::Path::new(&format!("/proc/{pid}")).exists()
                    && !std::path::Path::new(&scratch).exists()
            });
            if let Some(session) = session {
                assert!(session.control(Control::Play(true)).is_err());
            }
        }
    }
    #[test]
    fn missing_runtime_falls_back_but_document_errors_do_not() {
        for (reply, unavailable) in [
            (r#"{"Unavailable":"missing plugin"}"#, true),
            (r#"{"Error":"invalid media"}"#, false),
        ] {
            let (_root, source, worker) = fixture(&format!("printf '%s\\n' '{reply}'\n"));
            let result = WorkerSession::open_with_worker(
                worker,
                source,
                &CancelToken::new(),
                Arc::new(|| {}),
            );
            if unavailable {
                assert!(result.unwrap().is_none());
            } else {
                assert!(result.is_err());
            }
        }
    }
    #[test]
    fn an_unresponsive_worker_cannot_block_cleanup_on_a_full_command_pipe() {
        let (_root, source, worker) = fixture("printf '%s\\n' '\"Ready\"'\nsleep 30\n");
        let session = WorkerSession::open_with_worker(
            worker,
            source.clone(),
            &CancelToken::new(),
            Arc::new(|| {}),
        )
        .unwrap()
        .unwrap();
        let pid = std::fs::read_to_string(format!("{}.pid", source.display())).unwrap();
        until(|| {
            for _ in 0..32 {
                let _ = session.control(Control::Seek(1.));
            }
            session.take_update().error.is_some()
        });
        until(|| !std::path::Path::new(&format!("/proc/{pid}")).exists());
    }
    #[test]
    fn latest_frame_is_bounded_and_controls_reject_stale_frames() {
        let shared = Arc::new(Shared {
            update: Mutex::new(Update::default()),
            generation: AtomicU64::new(0),
            wake: Arc::new(|| {}),
        });
        let (commands, rx) = crossbeam_channel::bounded(1);
        let session = WorkerSession {
            shared: shared.clone(),
            commands,
            lifetime: CancelToken::new(),
        };
        for value in 0..100 {
            shared.publish(|u| {
                u.frame = Some(Frame {
                    width: 1,
                    height: 1,
                    bgra: vec![value; 4],
                });
            });
        }
        assert_eq!(session.take_update().frame.unwrap().bgra, vec![99; 4]);
        assert!(session.take_update().frame.is_none());
        shared.state(0, bibli_core::media::Playback::default());
        session.control(Control::Seek(3.)).unwrap();
        assert!(
            session.take_update().state.is_none(),
            "discard queued old position"
        );
        assert_eq!(shared.generation.load(Ordering::Acquire), 1);
        assert!(session.control(Control::Seek(4.)).is_err());
        assert_eq!(shared.generation.load(Ordering::Acquire), 1);
        assert!(matches!(
            rx.recv().unwrap(),
            Request::Control {
                generation: 1,
                command: Control::Seek(3.)
            }
        ));
        shared.frame(
            0,
            Frame {
                width: 1,
                height: 1,
                bgra: vec![1; 4],
            },
        );
        assert!(session.take_update().frame.is_none());
        shared.state(0, bibli_core::media::Playback::default());
        assert!(
            session.take_update().state.is_none(),
            "discard late old position"
        );
        shared.state(
            1,
            bibli_core::media::Playback {
                position: 3.,
                ..Default::default()
            },
        );
        assert!((session.take_update().state.unwrap().position - 3.).abs() < f64::EPSILON);
        shared.frame(
            1,
            Frame {
                width: 1,
                height: 1,
                bgra: vec![2; 4],
            },
        );
        assert_eq!(session.take_update().frame.unwrap().bgra, vec![2; 4]);
        assert!(session.control(Control::Rate(f64::NAN)).is_err());
    }
}
