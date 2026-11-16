//! Optional playback process. All process and pixel I/O lives on its manager;
//! UI methods only enqueue controls or take the latest bounded update.
//!
//! The manager thread sleeps in one `select!` over commands, worker replies,
//! its session's cancellation signal and, until the worker greets, a single
//! startup deadline. Nothing here polls on a timer.

use crate::line_channel::LineChannel;
use anyhow::Context;
use parking_lot::Mutex;
use skwir_core::{
    CancelToken,
    fs::kept_open::{KeptOpen, keep_open},
    location::Waker,
    media::{
        Control, Frame, MAX_MESSAGE, MediaSession, PIXELS_FILE, Playback, Reply, Request, Update,
    },
    op::PlanId,
    words,
    words::Words,
};
use std::{
    fs::OpenOptions,
    io::{Read, Seek, SeekFrom},
    path::PathBuf,
    process::Command,
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
    time::{Duration, Instant},
};

/// The one startup deadline: worker spawn, `GStreamer` initialization and the
/// first preroll. The worker has none of its own; cancelling the session
/// kills it.
const STARTUP_TIMEOUT: Duration = Duration::from_secs(20);

/// The player. Plugins that require it name this, not the literals.
pub const WORKER: crate::process::Worker = crate::process::Worker {
    variable: "SKWIR_MEDIA_WORKER",
    executable: "skwir-media-worker",
};

pub fn worker_path() -> Option<PathBuf> {
    WORKER.find()
}

struct Shared {
    update: Mutex<Update>,
    /// The latest control's, which the worker gives back with what it
    /// reports from the moment it carried it out.
    generation: AtomicU64,
    /// What the controls asked since made stale of what the worker
    /// reports (`Asked`); taken after `update`.
    asked: Mutex<Asked>,
    wake: Waker,
    /// Buffers of frames the window drew and gave back
    /// (`MediaSession::recycle`): the next frames are read into them, not
    /// into memory mapped and zeroed anew. Gone with the session.
    spares: Mutex<Vec<Vec<u8>>>,
}

/// Frame buffers a session keeps for its next frames: the one drawn, the
/// one taken, the one read.
const SPARES: usize = 2;

/// What is stale of what the worker reports, by the generation of the
/// control that made it so: a seek its frames and its state (the place
/// in the track); a pause, a rate or a track chosen its state; a volume
/// or a mute only that in the state, which a state from before it is
/// given as asked, its frames and the rest of it shown on: a slider
/// dragged sends one with every move, and dropping what came between
/// froze the picture and the time while it moved. Asking for frames or
/// none makes nothing stale.
#[derive(Default)]
struct Asked {
    frames_from: u64,
    state_from: u64,
    volume: Option<(u64, f64)>,
    mute: Option<(u64, bool)>,
}

impl Asked {
    /// `command`, sent as `generation`, makes stale what it changes; a
    /// state not taken yet, from before it, is given what it asked.
    fn control(&mut self, command: Control, generation: u64, update: &mut Update) {
        match command {
            Control::Seek(_) => {
                self.frames_from = generation;
                self.state_from = generation;
                update.frame = None;
                update.state = None;
            }
            Control::Play(_) | Control::Rate(_) | Control::Audio(_) | Control::Subtitle(_) => {
                self.state_from = generation;
                update.state = None;
            }
            Control::Volume(volume) => self.volume = Some((generation, volume)),
            Control::Mute(muted) => self.mute = Some((generation, muted)),
            Control::Frames(_) => {}
        }
        if let Some(state) = &mut update.state {
            self.give(generation.saturating_sub(1), state);
        }
    }

    /// A state the worker reported as `generation`, with what was asked
    /// since put in: none where it is stale.
    fn state(&self, generation: u64, mut state: Playback) -> Option<Playback> {
        if generation < self.state_from {
            return None;
        }
        self.give(generation, &mut state);
        Some(state)
    }

    fn give(&self, generation: u64, state: &mut Playback) {
        if let Some((at, volume)) = self.volume
            && generation < at
        {
            state.volume = volume;
        }
        if let Some((at, muted)) = self.mute
            && generation < at
        {
            state.muted = muted;
        }
    }
}

impl Shared {
    fn recycle(&self, bgra: Vec<u8>) {
        let mut spares = self.spares.lock();
        if bgra.capacity() > 0 && spares.len() < SPARES {
            spares.push(bgra);
        }
    }

    fn spare(&self) -> Option<Vec<u8>> {
        self.spares.lock().pop()
    }

    fn state(&self, generation: u64, state: Playback) {
        let mut update = self.update.lock();
        if generation > self.generation.load(Ordering::Acquire) {
            return;
        }
        let Some(state) = self.asked.lock().state(generation, state) else {
            return;
        };
        let empty = update.is_empty();
        update.state = Some(state);
        drop(update);
        if empty {
            (self.wake)();
        }
    }
    /// The worker went on to the queued file: what plays has a new name.
    fn advanced(&self, path: PathBuf) {
        let mut update = self.update.lock();
        let empty = update.is_empty();
        update.advanced = Some(path);
        drop(update);
        if empty {
            (self.wake)();
        }
    }
    /// A control the worker declined. Stale generations are dropped like state.
    fn notice(&self, generation: u64, message: String) {
        let mut update = self.update.lock();
        if generation > self.generation.load(Ordering::Acquire)
            || generation < self.asked.lock().state_from
        {
            return;
        }
        let empty = update.is_empty();
        update.notice = Some(message);
        drop(update);
        if empty {
            (self.wake)();
        }
    }
    /// Publishes a frame and returns a buffer the UI will never see: the
    /// rejected frame, or the one it superseded before the UI took it.
    fn frame(&self, generation: u64, frame: Frame) -> Vec<u8> {
        let mut update = self.update.lock();
        if generation > self.generation.load(Ordering::Acquire)
            || generation < self.asked.lock().frames_from
        {
            return frame.bgra;
        }
        let empty = update.is_empty();
        let previous = update.frame.replace(frame);
        drop(update);
        if empty {
            (self.wake)();
        }
        previous.map(|f| f.bgra).unwrap_or_default()
    }
    fn publish(&self, apply: impl FnOnce(&mut Update)) {
        let mut update = self.update.lock();
        let empty = update.is_empty();
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
/// Why a control was not taken: the player is busy, or has ended.
fn controls_busy() -> Words {
    words!(
        media_controls_busy = "video controls busy or session closed",
        note = "Why a play, pause or seek was not done: the player is still taking the ones before, or it has stopped."
    )
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
            .context(controls_busy())?;
        self.shared.generation.store(generation, Ordering::Release);
        self.shared
            .asked
            .lock()
            .control(command, generation, &mut update);
        Ok(())
    }
    fn play_next(&self, path: Option<PathBuf>) -> anyhow::Result<()> {
        anyhow::ensure!(!self.lifetime.is_cancelled(), "video session closed");
        self.commands
            .try_send(Request::Next { path })
            .context(controls_busy())
    }
    fn take_update(&self) -> Update {
        std::mem::take(&mut *self.shared.update.lock())
    }
    fn recycle(&self, bgra: Vec<u8>) {
        self.shared.recycle(bgra);
    }
}
impl WorkerSession {
    /// Open a player on `path`, with how the worker should treat the sound:
    /// settings it needs when it builds its pipeline, not while it plays.
    /// `None` when the worker or its runtime is not installed.
    pub fn open_tuned(
        path: PathBuf,
        tuning: Tuning,
        lifetime: &CancelToken,
        wake: Waker,
    ) -> anyhow::Result<Option<Arc<dyn MediaSession>>> {
        let Some(executable) = worker_path() else {
            return Ok(None);
        };
        Self::open_with_tuning(executable, path, tuning, lifetime, wake)
    }

    #[cfg(all(test, target_os = "linux"))]
    fn open_with_worker(
        executable: PathBuf,
        path: PathBuf,
        lifetime: &CancelToken,
        wake: Waker,
    ) -> anyhow::Result<Option<Arc<dyn MediaSession>>> {
        Self::open_with_tuning(executable, path, Tuning::default(), lifetime, wake)
    }

    fn open_with_tuning(
        executable: PathBuf,
        path: PathBuf,
        tuning: Tuning,
        lifetime: &CancelToken,
        wake: Waker,
    ) -> anyhow::Result<Option<Arc<dyn MediaSession>>> {
        let lifetime = lifetime.child();
        let shared = Arc::new(Shared {
            update: Mutex::new(Update::default()),
            generation: AtomicU64::new(0),
            asked: Mutex::default(),
            wake,
            spares: Mutex::default(),
        });
        let (commands, rx) = crossbeam_channel::bounded(32);
        let (startup, ready) = crossbeam_channel::bounded(1);
        let manager_shared = shared.clone();
        let manager_lifetime = lifetime.clone();
        std::thread::Builder::new()
            .name("skwir-video-session".into())
            .spawn(move || {
                let helper = executable.clone();
                // The file played is kept open until the worker is gone,
                // and let go of when a plan takes it (ADR-136).
                let mut kept = Kept::new(path.clone());
                let result = manage(
                    executable,
                    path,
                    tuning,
                    &manager_lifetime,
                    &mut kept,
                    &manager_shared,
                    &rx,
                    &startup,
                );
                // The worker reaped: the plan waiting for its file goes on.
                let taken = kept.taken();
                drop(kept);
                if let Some(plan) = taken {
                    manager_shared.publish(|u| u.taken = Some(plan));
                } else if let Err(error) = result {
                    let error =
                        Arc::new(crate::process::explain(&helper, &manager_lifetime, error));
                    let _ = startup.try_send(Err(Words::of_error_from(&error, 0).into()));
                    if !manager_lifetime.is_cancelled() {
                        // Transitional: an update's error is text, said
                        // once here (ADR-139).
                        let said = Words::of_error_from(&error, 0).say();
                        manager_shared.publish(|u| u.error = Some(said));
                    }
                }
                manager_lifetime.cancel();
            })?;
        // The manager owns the startup deadline; this only waits for its
        // verdict or for the session to be cancelled underneath it.
        let cancelled = lifetime.subscribe();
        crossbeam_channel::select! {
            recv(ready) -> verdict => match verdict {
                Ok(Ok(true)) => Ok(Some(Arc::new(Self {
                    commands,
                    shared,
                    lifetime,
                }))),
                Ok(Ok(false)) => Ok(None),
                Ok(Err(error)) => Err(error),
                Err(_) => anyhow::bail!(words!(
                    media_worker_stopped = "video worker stopped during startup",
                    note = "Why a film or a sound does not play: the program that plays it stopped before it was ready."
                )),
            },
            recv(cancelled.receiver()) -> _ => {
                // The manager cancels its lifetime right after sending its
                // verdict: when both are ready, the verdict is the outcome.
                if let Ok(verdict) = ready.try_recv() {
                    lifetime.cancel();
                    return match verdict {
                        Ok(false) => Ok(None),
                        Ok(true) => anyhow::bail!("video open cancelled"),
                        Err(error) => Err(error),
                    };
                }
                lifetime.cancel();
                anyhow::bail!("video open cancelled")
            }
        }
    }
}

/// How loud tracks are made to sound alike, from the gain their tags carry.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ReplayGain {
    #[default]
    Off,
    /// Every track to the same loudness.
    Track,
    /// An album keeps its own dynamics: quiet songs stay quiet.
    Album,
}

impl ReplayGain {
    pub fn from_setting(value: &str) -> Self {
        match value {
            "track" => Self::Track,
            "album" => Self::Album,
            _ => Self::Off,
        }
    }
}

#[derive(Clone, Copy, Debug, Default)]
pub struct Tuning {
    pub replay_gain: ReplayGain,
}

/// The files a session's worker keeps open (`fs::kept_open`): the one it
/// plays, and the one it was handed to go on with once it opened it
/// (`Reply::Handing`, as the first nears its end). A plan that takes
/// either ends the session, told which plan. The file queued and not
/// handed over is not open: a plan that takes it has it un-queued
/// (`Request::Next` with none), the track playing on, and the pin tells
/// the queue again once the plan has ended.
struct Kept {
    playing: KeptOpen,
    /// Queued, not opened yet: let go of by being un-queued.
    queued: Option<KeptOpen>,
    /// Handed over, open beside the one playing until it takes over.
    handing: Option<KeptOpen>,
    let_go: CancelToken,
    taken: Arc<Mutex<Option<PlanId>>>,
    /// What a plan let go of the queued file for, to the manager.
    unqueue: (
        crossbeam_channel::Sender<PlanId>,
        crossbeam_channel::Receiver<PlanId>,
    ),
}

impl Kept {
    fn new(path: PathBuf) -> Self {
        let let_go = CancelToken::new();
        let taken = Arc::new(Mutex::new(None));
        let (ends, by) = (let_go.clone(), Arc::clone(&taken));
        let playing = keep_open(path, move |plan| {
            by.lock().get_or_insert(plan);
            ends.cancel();
        });
        Kept {
            playing,
            queued: None,
            handing: None,
            let_go,
            taken,
            unqueue: crossbeam_channel::unbounded(),
        }
    }

    /// The file to go on with, or none: a plan that takes it has it
    /// un-queued, not the session ended.
    fn queue(&mut self, path: Option<&PathBuf>) {
        self.queued = path.map(|path| {
            let told = self.unqueue.0.clone();
            keep_open(path.clone(), move |plan| {
                let _ = told.send(plan);
            })
        });
    }

    /// The worker opened the file it was handed to go on with: open from
    /// now on, as the one playing is. A plan taking it already (it was
    /// handed over as the plan began) has it: the session ends for it.
    fn handing(&mut self, path: &std::path::Path) -> Option<PlanId> {
        self.queued = None;
        if let Some(plan) = skwir_core::fs::kept_open::taken_by(path) {
            self.taken.lock().get_or_insert(plan);
            return Some(plan);
        }
        self.handing = self.playing.also(path.to_path_buf());
        None
    }

    /// The worker went on to the file handed over.
    fn advanced(&mut self, path: &std::path::Path) {
        self.playing.now_at(path.to_path_buf());
        self.handing = None;
    }

    /// The plan that took the file, if one did.
    fn taken(&self) -> Option<PlanId> {
        *self.taken.lock()
    }
}

// Each is what the manager works with; a struct of them would be
// unpacked at its one call.
#[allow(clippy::too_many_arguments)]
fn manage(
    executable: PathBuf,
    path: PathBuf,
    tuning: Tuning,
    lifetime: &CancelToken,
    kept: &mut Kept,
    shared: &Shared,
    commands: &crossbeam_channel::Receiver<Request>,
    startup: &crossbeam_channel::Sender<anyhow::Result<bool>>,
) -> anyhow::Result<()> {
    let scratch = frame_slot_dir()?;
    let mut options = OpenOptions::new();
    options.read(true).write(true).create_new(true);
    // Windows writes a file's changed pages out a second apart; one marked
    // temporary stays in memory while there is room, so the frames
    // written over it thirty times a second never reach the disk.
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt as _;
        const FILE_ATTRIBUTE_TEMPORARY: u32 = 0x100;
        options.attributes(FILE_ATTRIBUTE_TEMPORARY);
    }
    let mut pixels = options.open(scratch.path().join(PIXELS_FILE))?;
    pixels.set_len(u64::try_from(skwir_core::media::frame_bytes(
        skwir_core::media::MAX_SIDE,
        skwir_core::media::MAX_SIDE,
    )?)?)?;
    // By the bytes where the name is not enough: an `.ogg` may hold a film.
    let audio_only = path.file_name().is_some_and(|name| {
        crate::mime::identify(name, &path).is_some_and(|mime| mime.0.starts_with("audio/"))
    });
    let mut command = Command::new(executable);
    command.arg(path).arg(scratch.path());
    if audio_only {
        command.env("SKWIR_MEDIA_AUDIO_ONLY", "1");
    }
    match tuning.replay_gain {
        ReplayGain::Off => {}
        ReplayGain::Track => {
            command.env("SKWIR_MEDIA_REPLAYGAIN", "track");
        }
        ReplayGain::Album => {
            command.env("SKWIR_MEDIA_REPLAYGAIN", "album");
        }
    }
    // Decoder/overlay threads are recreated during track changes. glibc's
    // CPU-count-based arena limit otherwise retains hundreds of MB of freed
    // frame buffers across those threads. Bound fragmentation in this child.
    #[cfg(target_env = "gnu")]
    command.env("MALLOC_ARENA_MAX", "2");
    let mut worker = LineChannel::<Reply>::spawn(&mut command, MAX_MESSAGE, "skwir-media-worker")?;
    let cancelled = lifetime.subscribe();
    let let_go = kept.let_go.subscribe();
    let unqueue = kept.unqueue.1.clone();
    // Fires once; after the greeting it is simply never selected again.
    let deadline = crossbeam_channel::at(Instant::now() + STARTUP_TIMEOUT);
    let mut ready = false;
    // Buffers the UI has not taken come back here, so a UI slower than the
    // decoder recycles one allocation instead of allocating per frame.
    let mut spare: Vec<u8> = Vec::new();
    while !lifetime.is_cancelled() && !kept.let_go.is_cancelled() {
        let reply = crossbeam_channel::select! {
            recv(commands) -> request => {
                let Ok(request) = request else { break; };
                if let Request::Next { path } = &request {
                    kept.queue(path.as_ref());
                }
                worker.send(&request)?;
                continue;
            }
            recv(worker.replies()) -> reply => {
                let result = reply
                    .map_err(|_| {
                        anyhow::Error::from(words!(
                            media_worker_exited = "video worker exited",
                            note = "Why a film or a sound stopped playing: the program that plays it stopped."
                        ))
                    })
                    .and_then(|r| r);
                if result.is_err() && !ready && worker.missing_runtime() {
                    // Decoder/file errors have a protocol reply and never fall back.
                    let _ = startup.send(Ok(false));
                    return Ok(());
                }
                result?
            },
            recv(cancelled.receiver()) -> _ => break,
            recv(let_go.receiver()) -> _ => break,
            // A plan takes the file queued: the worker forgets it before
            // the plan goes on, the track playing on (ADR-136).
            // Told first, then let go of: one handed over meanwhile is
            // said so (`Reply::Handing`), and ends the session for it.
            recv(unqueue) -> _ => {
                if kept.queued.is_some() {
                    worker.send(&Request::Next { path: None })?;
                    kept.queued = None;
                }
                continue;
            }
            recv(deadline) -> _ => {
                anyhow::ensure!(
                    ready,
                    words!(
                        media_worker_timed_out = "video worker timed out",
                        note = "Why a film or a sound does not play: the program that plays it took too long to start."
                    )
                );
                continue;
            }
        };
        match reply {
            Reply::Ready => {
                anyhow::ensure!(!ready, "duplicate video greeting");
                ready = true;
                let _ = startup.send(Ok(true));
            }
            Reply::Unavailable(message) => {
                // Before the greeting this selects the stills fallback; a
                // running session losing its runtime is an error, not silence.
                anyhow::ensure!(!ready, "{message}");
                let _ = startup.send(Ok(false));
                return Ok(());
            }
            Reply::Rejected {
                generation,
                message,
            } => shared.notice(generation, message),
            Reply::Error(error) => anyhow::bail!("{error}"),
            Reply::Handing { path } => {
                if kept.handing(&path).is_some() {
                    break;
                }
            }
            Reply::Advanced { path } => {
                kept.advanced(&path);
                shared.advanced(path);
            }
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
                let bytes = skwir_core::media::frame_bytes(width, height)?;
                let mut bgra = std::mem::take(&mut spare);
                // A buffer the window gave back, when this one is too small.
                if bgra.capacity() < bytes
                    && let Some(given) = shared.spare()
                {
                    bgra = given;
                }
                pixels.seek(SeekFrom::Start(0))?;
                // Read into room set aside, never zeroed first: the window
                // keeps each frame it takes, so most come into a new buffer,
                // and zeroing 8 MB at 1080p before reading over it, thirty
                // times a second, was work for nothing.
                bgra.clear();
                bgra.reserve_exact(bytes);
                (&mut pixels).take(bytes as u64).read_to_end(&mut bgra)?;
                anyhow::ensure!(bgra.len() == bytes, "a video frame came short");
                spare = shared.frame(
                    generation,
                    Frame {
                        width,
                        height,
                        bgra,
                    },
                );
                worker.send(&Request::Consumed)?;
            }
        }
    }
    // The channel reaps its child before scratch is removed. Neither runs on the UI.
    Ok(())
}

/// One private reusable slot; never one file per frame. RAM-backed through
/// `/dev/shm` where that exists and is writable, otherwise the ordinary
/// temporary directory, which may be disk-backed.
fn frame_slot_dir() -> anyhow::Result<tempfile::TempDir> {
    crate::file::memory_dir("skwir-video-").context(words!(
        media_frame_slot = "create video frame slot",
        note = "What could not be done before a film plays, before the system's reason: a place in memory for its pictures."
    ))
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
    /// Frames asked off (the window not shown) and on again reach the
    /// worker as controls of their own, the session kept.
    #[test]
    fn frames_off_and_on_reach_the_worker() {
        let (_root, source, worker) = fixture(
            "printf '%s\\n' '\"Ready\"'\nwhile IFS= read -r line; do printf '%s\\n' \"$line\" >> \"$1.requests\"; done\n",
        );
        let session = WorkerSession::open_with_worker(
            worker,
            source.clone(),
            &CancelToken::new(),
            Arc::new(|| {}),
        )
        .unwrap()
        .unwrap();
        session.control(Control::Frames(false)).unwrap();
        session.control(Control::Frames(true)).unwrap();
        let requests = format!("{}.requests", source.display());
        let mut said = String::new();
        until(|| {
            said = std::fs::read_to_string(&requests).unwrap_or_default();
            said.lines().count() == 2
        });
        let lines: Vec<&str> = said.lines().collect();
        assert!(lines[0].contains(r#""command":{"Frames":false}"#), "{said}");
        assert!(lines[1].contains(r#""command":{"Frames":true}"#), "{said}");
        assert!(session.take_update().error.is_none());
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
    /// A plan that takes the file played, or the one queued after it once
    /// the worker has opened it, ends the session (ADR-136): the worker is
    /// gone before the plan goes on, and the session says which plan took
    /// it, not that it failed.
    #[test]
    fn a_plan_taking_the_file_played_ends_the_session_first() {
        for handed in [false, true] {
            // Handed over by itself, as playbin asks at a track's end: the
            // file queued and not handed is un-queued instead (below), so a
            // plan that came before the word would take nothing here.
            let (root, source, worker) = fixture(
                "printf '%s\\n' '\"Ready\"'\ncat \"$1.handing\"\nwhile IFS= read -r line; do :; done\n",
            );
            let next = root.path().join("the next one's.ogg");
            let handing = if handed {
                serde_json::to_string(&Reply::Handing { path: next.clone() }).unwrap() + "\n"
            } else {
                String::new()
            };
            std::fs::write(format!("{}.handing", source.display()), handing).unwrap();
            let session = WorkerSession::open_with_worker(
                worker,
                source.clone(),
                &CancelToken::new(),
                Arc::new(|| {}),
            )
            .unwrap()
            .unwrap();
            let pid = std::fs::read_to_string(format!("{}.pid", source.display())).unwrap();
            let taken = if handed { next } else { source.clone() };
            let plan = PlanId(41);
            until(|| {
                // The file handed over is kept once the manager has heard so.
                skwir_core::fs::kept_open::let_go_of(
                    plan,
                    |path| path == taken,
                    Duration::from_secs(3),
                )
                .count()
                    == 1
            });
            assert!(
                !std::path::Path::new(&format!("/proc/{pid}")).exists(),
                "reaped before the plan goes on"
            );
            let mut update = Update::default();
            until(|| {
                update = session.take_update();
                !update.is_empty()
            });
            assert_eq!((update.taken, update.error), (Some(plan), None));
            assert!(session.control(Control::Play(true)).is_err());
        }
    }

    /// The file queued and not handed over yet is not open: a plan that
    /// takes it has the worker forget it, and the track plays on.
    #[test]
    fn a_plan_taking_the_file_queued_has_it_unqueued() {
        let (root, source, worker) = fixture(
            "printf '%s\\n' '\"Ready\"'\nwhile IFS= read -r line; do printf '%s\\n' \"$line\" >> \"$1.lines\"; done\n",
        );
        let session = WorkerSession::open_with_worker(
            worker,
            source.clone(),
            &CancelToken::new(),
            Arc::new(|| {}),
        )
        .unwrap()
        .unwrap();
        let pid = std::fs::read_to_string(format!("{}.pid", source.display())).unwrap();
        let next = root.path().join("the next one's.ogg");
        session.play_next(Some(next.clone())).unwrap();
        until(|| {
            skwir_core::fs::kept_open::let_go_of(
                PlanId(42),
                |path| path == next,
                Duration::from_secs(3),
            )
            .count()
                == 1
        });
        let lines = format!("{}.lines", source.display());
        let unqueued = serde_json::to_string(&Request::Next { path: None }).unwrap();
        until(|| {
            std::fs::read_to_string(&lines)
                .is_ok_and(|told| told.lines().any(|line| line == unqueued))
        });
        assert!(
            std::path::Path::new(&format!("/proc/{pid}")).exists(),
            "the track plays on"
        );
        assert_eq!(session.take_update().taken, None);
        session.control(Control::Play(true)).unwrap();
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
    fn rejected_controls_surface_a_notice_and_keep_the_session() {
        // A non-seekable stream declines Seek; the worker answers Rejected and
        // then reports its unchanged state under the same generation.
        let playback = r#"{"status":"Paused","position":0.0,"duration":null,"volume":1.0,"muted":false,"rate":1.0,"audio_tracks":[],"subtitle_tracks":[],"audio":-1,"subtitle":-1,"seekable":false}"#;
        let (_root, source, worker) = fixture(&format!(
            "printf '%s\\n' '\"Ready\"'\nIFS= read -r line\nprintf '%s\\n' '{{\"Rejected\":{{\"generation\":1,\"message\":\"this video does not support seeking\"}}}}'\nprintf '%s\\n' '{{\"State\":{{\"generation\":1,\"playback\":{playback}}}}}'\nwhile IFS= read -r line; do :; done\n"
        ));
        let session = WorkerSession::open_with_worker(
            worker,
            source.clone(),
            &CancelToken::new(),
            Arc::new(|| {}),
        )
        .unwrap()
        .unwrap();
        let pid = std::fs::read_to_string(format!("{}.pid", source.display())).unwrap();
        session.control(Control::Seek(5.)).unwrap();
        let mut notice = None;
        let mut state = None;
        until(|| {
            let update = session.take_update();
            assert!(update.error.is_none(), "a rejected control is not fatal");
            notice = notice.take().or(update.notice);
            state = state.take().or(update.state);
            notice.is_some() && state.is_some()
        });
        assert_eq!(
            notice.as_deref(),
            Some("this video does not support seeking")
        );
        assert!(!state.unwrap().seekable);
        assert!(
            std::path::Path::new(&format!("/proc/{pid}")).exists(),
            "worker survives a rejected control"
        );
        session.control(Control::Play(true)).unwrap();
        assert!(session.take_update().error.is_none());
    }
    #[test]
    fn losing_the_runtime_after_ready_is_an_error() {
        let (_root, source, worker) = fixture(
            "printf '%s\\n' '\"Ready\"'\nprintf '%s\\n' '{\"Unavailable\":\"runtime gone\"}'\nsleep 30\n",
        );
        let session = WorkerSession::open_with_worker(
            worker,
            source.clone(),
            &CancelToken::new(),
            Arc::new(|| {}),
        )
        .unwrap()
        .unwrap();
        let pid = std::fs::read_to_string(format!("{}.pid", source.display())).unwrap();
        let mut error = None;
        until(|| {
            error = error.take().or(session.take_update().error);
            error.is_some()
        });
        assert_eq!(error.as_deref(), Some("runtime gone"));
        until(|| !std::path::Path::new(&format!("/proc/{pid}")).exists());
        assert!(session.control(Control::Play(true)).is_err());
    }
    #[test]
    fn cancellation_wakes_the_manager_without_a_poll_interval() {
        // The manager used to notice cancellation on a 100 ms poll. Reaping an
        // idle worker now completes well inside that interval, repeatedly.
        for _ in 0..4 {
            let (_root, source, worker) = fixture("printf '%s\\n' '\"Ready\"'\nsleep 30\n");
            let lifetime = CancelToken::new();
            let session =
                WorkerSession::open_with_worker(worker, source.clone(), &lifetime, Arc::new(|| {}))
                    .unwrap()
                    .unwrap();
            let pid = std::fs::read_to_string(format!("{}.pid", source.display())).unwrap();
            let proc = format!("/proc/{pid}");
            let started = Instant::now();
            lifetime.cancel();
            while std::path::Path::new(&proc).exists() {
                assert!(
                    started.elapsed() < Duration::from_millis(50),
                    "cancellation waited for a poll interval"
                );
                std::thread::sleep(Duration::from_millis(1));
            }
            drop(session);
        }
        // Cancelling during startup returns from open without its deadline.
        let (_root, source, worker) = fixture("sleep 30\n");
        let lifetime = CancelToken::new();
        let canceller = lifetime.clone();
        std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(20));
            canceller.cancel();
        });
        let started = Instant::now();
        let result = WorkerSession::open_with_worker(worker, source, &lifetime, Arc::new(|| {}));
        assert!(result.is_err());
        assert!(started.elapsed() < Duration::from_secs(2));
    }
    #[test]
    fn untaken_frame_buffers_are_recycled() {
        let shared = Shared {
            update: Mutex::new(Update::default()),
            generation: AtomicU64::new(0),
            asked: Mutex::default(),
            wake: Arc::new(|| {}),
            spares: Mutex::default(),
        };
        let frame = |value: u8| Frame {
            width: 1,
            height: 1,
            bgra: vec![value; 4],
        };
        assert!(
            shared.frame(0, frame(1)).is_empty(),
            "nothing to recycle yet"
        );
        assert_eq!(shared.frame(0, frame(2)), vec![1; 4], "superseded frame");
        assert_eq!(shared.frame(9, frame(3)), vec![3; 4], "stale generation");
        assert_eq!(
            std::mem::take(&mut *shared.update.lock())
                .frame
                .unwrap()
                .bgra,
            vec![2; 4]
        );
        assert!(shared.frame(0, frame(4)).is_empty(), "taken by the UI");
        // What the window drew and gave back is kept for the next frames,
        // a few, and only buffers that hold something.
        shared.recycle(Vec::new());
        assert!(shared.spare().is_none(), "an empty buffer is no room");
        for value in 5..9 {
            shared.recycle(vec![value; 4]);
        }
        assert_eq!(shared.spare(), Some(vec![6; 4]));
        assert_eq!(shared.spare(), Some(vec![5; 4]));
        assert!(shared.spare().is_none(), "at most SPARES kept");
    }
    /// A volume dragged sends a control with every move: what the worker
    /// reports meanwhile is shown on, its frames and its time, the volume
    /// in it as asked until the worker has carried that out. A pause makes
    /// its state stale, not its frames; a seek both.
    #[test]
    fn a_volume_dragged_keeps_frames_and_time_coming() {
        let shared = Arc::new(Shared {
            update: Mutex::new(Update::default()),
            generation: AtomicU64::new(0),
            asked: Mutex::default(),
            wake: Arc::new(|| {}),
            spares: Mutex::default(),
        });
        let (commands, _rx) = crossbeam_channel::bounded(16);
        let session = WorkerSession {
            shared: shared.clone(),
            commands,
            lifetime: CancelToken::new(),
        };
        let frame = |value: u8| Frame {
            width: 1,
            height: 1,
            bgra: vec![value; 4],
        };
        let at = |position: f64, volume: f64| Playback {
            position,
            volume,
            ..Playback::default()
        };
        shared.frame(0, frame(1));
        session.control(Control::Volume(0.5)).unwrap();
        shared.state(0, at(1., 0.2));
        session.control(Control::Volume(0.6)).unwrap();
        let update = session.take_update();
        assert_eq!(update.frame.unwrap().bgra, vec![1; 4], "the frame kept");
        let state = update.state.unwrap();
        assert!(
            (state.position - 1.).abs() < f64::EPSILON,
            "the time goes on"
        );
        assert!((state.volume - 0.6).abs() < f64::EPSILON, "as asked");
        shared.frame(1, frame(2));
        shared.state(1, at(2., 0.5));
        session.control(Control::Mute(true)).unwrap();
        let update = session.take_update();
        assert_eq!(update.frame.unwrap().bgra, vec![2; 4]);
        let state = update.state.unwrap();
        assert!((state.volume - 0.6).abs() < f64::EPSILON && state.muted);
        // Carried out: what the worker reports is shown as it says.
        shared.state(3, at(3., 0.4));
        let state = session.take_update().state.unwrap();
        assert!((state.volume - 0.4).abs() < f64::EPSILON && !state.muted);

        session.control(Control::Play(false)).unwrap();
        shared.state(3, at(3.5, 0.4));
        shared.frame(3, frame(3));
        let update = session.take_update();
        assert!(update.state.is_none(), "the state before a pause is stale");
        assert_eq!(update.frame.unwrap().bgra, vec![3; 4], "its frames are not");
        session.control(Control::Seek(9.)).unwrap();
        shared.state(4, at(4., 0.4));
        shared.frame(4, frame(4));
        assert!(
            session.take_update().is_empty(),
            "nothing from before a seek"
        );
        shared.frame(5, frame(5));
        assert_eq!(session.take_update().frame.unwrap().bgra, vec![5; 4]);
    }

    #[test]
    fn latest_frame_is_bounded_and_controls_reject_stale_frames() {
        let shared = Arc::new(Shared {
            update: Mutex::new(Update::default()),
            generation: AtomicU64::new(0),
            asked: Mutex::default(),
            wake: Arc::new(|| {}),
            spares: Mutex::default(),
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
        shared.state(0, skwir_core::media::Playback::default());
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
        shared.state(0, skwir_core::media::Playback::default());
        assert!(
            session.take_update().state.is_none(),
            "discard late old position"
        );
        shared.state(
            1,
            skwir_core::media::Playback {
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
